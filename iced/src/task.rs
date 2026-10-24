//! Durable cloneable application task plans.

use rawr::{application, host, item, preview, program, Dynamic, EvalAltResult, Position};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::{dispatch, effect, runtime};

/// What happens to one output once it reaches the UI thread: the rest of
/// the plan's mappers and checks, ending in a stamped message.
type Next = Arc<dyn Fn(Dynamic) -> iced::Task<runtime::Message> + Send + Sync>;

/// The owner and authority a plan is lowered under.
#[derive(Clone)]
struct Lowering {
    owner: effect::Owner,
    authority: Authority,
}

/// A cloneable description of work to lower at an iced application boundary.
#[derive(Clone, Debug)]
pub struct Plan {
    kind: Kind,
    witness: Option<host::runtime::Witness>,
}

pub(crate) struct Prepared {
    kind: PreparedKind,
    witness: Option<host::runtime::Witness>,
}

enum PreparedKind {
    None,
    Done(Dynamic),
    Perform {
        pending: rawr::reload::installation::operation::Prepared,
    },
    Batch(Box<[Prepared]>),
    Chain {
        first: Box<Prepared>,
        second: Box<Prepared>,
    },
    Map {
        plan: Box<Prepared>,
        mapper: item::Value,
    },
    Native(Native),
    AndThen {
        plan: Box<Prepared>,
        mapper: item::Value,
    },
    Imported(Imported),
    Discard(Box<Prepared>),
    Sip {
        sipper: Sip,
        on_progress: item::Value,
        on_output: item::Value,
    },
    Abortable {
        plan: Box<Prepared>,
        abort: Arc<Abort>,
    },
    Script(item::Value),
    Send { sender: Sender, value: Dynamic },
    Join { plans: Box<[Prepared]>, fallible: bool },
    Select(Box<[Prepared]>),
}

/// One value a Rust task yields, turned into a Rawr value on the UI thread.
/// The witness is the one for the task's output type.
#[doc(hidden)]
pub type Materialize =
    Box<dyn FnOnce(Option<&host::runtime::Witness>) -> Result<Dynamic, String> + Send + 'static>;

/// An iced `Task` a Rust function returned. It runs once: Rust's `Task` is
/// not `Clone`, and a copy of the plan shares the one native task.
#[derive(Clone)]
pub(crate) struct Imported(Arc<Mutex<Option<iced::Task<Materialize>>>>);

impl Imported {
    fn new(task: iced::Task<Materialize>) -> Self {
        Self(Arc::new(Mutex::new(Some(task))))
    }

    fn take(&self) -> Option<iced::Task<Materialize>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl fmt::Debug for Imported {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Imported(<native task>)")
    }
}

/// A Rust function's output on its way into the facade's `Task`: either an
/// iced `Task<T>` or a `Future<Output = T>`. Generated bridge code calls
/// `(&Project::new(output)).into_task()` with [`ViaTask`] and [`ViaFuture`]
/// in scope; method resolution picks the task impl first.
#[doc(hidden)]
pub struct Project<T>(std::cell::Cell<Option<T>>);

impl<T> Project<T> {
    #[must_use]
    pub fn new(value: T) -> Self {
        Self(std::cell::Cell::new(Some(value)))
    }

    fn take(&self) -> T {
        self.0
            .take()
            .expect("a projected Rust output is taken once")
    }
}

/// An iced `Task` crosses as it is, actions and all.
#[doc(hidden)]
pub trait ViaTask<T> {
    fn into_task(&self) -> iced::Task<T>;
}

impl<T> ViaTask<T> for Project<iced::Task<T>> {
    fn into_task(&self) -> iced::Task<T> {
        self.take()
    }
}

/// A Rust future crosses as iced's `Task::future`.
#[doc(hidden)]
pub trait ViaFuture<T> {
    fn into_task(&self) -> iced::Task<T>;
}

impl<F> ViaFuture<F::Output> for &Project<F>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn into_task(&self) -> iced::Task<F::Output> {
        iced::Task::future(self.take())
    }
}

/// One step of a Rust sipper: a progress value, or its output.
#[doc(hidden)]
pub enum Step {
    Progress(Materialize),
    Output(Materialize),
}

/// A Rust `impl Sipper<Output, Progress>`, which the facade names
/// `task::Sipper<Output, Progress>`. It runs once, as Rust's does.
#[derive(Clone)]
pub struct Sip {
    task: Arc<Mutex<Option<iced::Task<Step>>>>,
    /// A script sipper's builder, `async |progress| ..`, in place of a
    /// Rust task.
    script: Option<item::Value>,
    witness: Option<host::runtime::Witness>,
}

impl Sip {
    fn take(&self) -> Option<iced::Task<Step>> {
        self.task.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// The witness of the sipper's `Output` (0) or `Progress` (1).
    fn witness(&self, index: usize) -> Option<host::runtime::Witness> {
        self.witness
            .as_ref()
            .and_then(|witness| witness.argument_witness(index))
    }

    /// The sipper's progress as a stream, for `Subscription::run`. Each
    /// value becomes a Rawr value on the UI thread. Its output ends it.
    pub(crate) fn progress(
        &self,
    ) -> Option<iced::futures::stream::BoxStream<'static, Dynamic>> {
        use iced::futures::StreamExt as _;
        let stream = iced_runtime::task::into_stream(self.take()?)?;
        let witness = self.witness(1);
        Some(
            stream
                .filter_map(move |action| {
                    let witness = witness.clone();
                    async move {
                        match action {
                            iced_runtime::Action::Output(Step::Progress(materialize)) => {
                                Some(Dynamic::from(host::runtime::Deferred::new(move || {
                                    materialize(witness.as_ref())
                                })))
                            }
                            _ => None,
                        }
                    }
                })
                .boxed(),
        )
    }
}

/// iced's `task::Sender` that a script sipper's builder sends its progress
/// through: each value goes to the `Task::sip` that runs the sipper.
#[derive(Clone)]
pub struct Sender(Arc<dyn Fn(Dynamic) -> iced::Task<runtime::Message> + Send + Sync>);

impl fmt::Debug for Sender {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Sender")
    }
}

impl fmt::Debug for Sip {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Sipper(<native>)")
    }
}

/// Project a Rust sipper into the facade's `task::Sipper`, as iced's
/// `Task::sip` runs one. The bridge stamps it with the call's witness.
#[doc(hidden)]
#[must_use]
pub fn sipper<S, O, P>(
    sipper: S,
    mut on_progress: impl FnMut(P) -> Materialize + Send + 'static,
    on_output: impl FnOnce(O) -> Materialize + Send + 'static,
) -> Dynamic
where
    S: iced::task::Sipper<O, P> + Send + 'static,
    O: Send + 'static,
    P: Send + 'static,
{
    let task = iced::Task::sip(
        sipper,
        move |progress| Step::Progress(on_progress(progress)),
        move |output| Step::Output(on_output(output)),
    );
    let task = Arc::new(Mutex::new(Some(task)));
    Dynamic::from(host::runtime::Unwitnessed::new(move |witness| {
        Dynamic::from(Sip {
            task,
            script: None,
            witness: Some(witness),
        })
    }))
}

/// iced's `task::Handle`: aborts the task `Task::abortable` returned it
/// with. `abort_on_drop` gives one that aborts when its last copy drops.
#[derive(Clone)]
pub struct Handle {
    abort: Arc<Abort>,
    guard: Option<Arc<Guard>>,
}

/// The native abort handle of one abortable plan, once it is lowered.
#[derive(Default)]
pub(crate) struct Abort {
    aborted: std::sync::atomic::AtomicBool,
    native: Mutex<Option<iced::task::Handle>>,
}

struct Guard(Arc<Abort>);

thread_local! {
    /// The handles of the abortable plans lowered in `collect_aborts`.
    static LOWERED: std::cell::RefCell<Option<Vec<Arc<Abort>>>> =
        const { std::cell::RefCell::new(None) };
}

/// Lower with `lower`, and the handles of the abortable plans it lowered,
/// which the task's owner aborts when it retires the task.
pub(crate) fn collect_aborts<R>(lower: impl FnOnce() -> R) -> (R, Vec<Arc<Abort>>) {
    let outer = LOWERED.with(|lowered| lowered.replace(Some(Vec::new())));
    let result = lower();
    let collected = LOWERED
        .with(|lowered| lowered.replace(outer))
        .unwrap_or_default();
    (result, collected)
}

impl Abort {
    pub(crate) fn abort(&self) {
        self.aborted.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(handle) = self.native.lock().unwrap_or_else(PoisonError::into_inner).take() {
            handle.abort();
        }
    }

    fn is_aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Make a lowered task abortable through this handle.
    fn attach(self: &Arc<Self>, task: iced::Task<runtime::Message>) -> iced::Task<runtime::Message> {
        LOWERED.with(|lowered| {
            if let Some(lowered) = lowered.borrow_mut().as_mut() {
                lowered.push(self.clone());
            }
        });
        let (task, handle) = task.abortable();
        if self.is_aborted() {
            handle.abort();
        } else {
            *self.native.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
        }
        task
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Handle")
            .field("aborted", &self.abort.is_aborted())
            .finish()
    }
}

impl fmt::Debug for Abort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Abort")
    }
}

/// A task that calls a Rust function returning a future when it first
/// runs, and then awaits the future. The call happens on iced's executor,
/// so a future that needs its runtime, as tokio's `sleep` does, finds it.
#[doc(hidden)]
pub fn lazy<F>(make: impl FnOnce() -> F + Send + 'static) -> iced::Task<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    iced::Task::future(async move { make().await })
}

/// Project an iced `Task` from a Rust function into the facade's `Task`.
/// The bridge stamps it with the call's output witness.
#[doc(hidden)]
#[must_use]
pub fn imported(task: iced::Task<Materialize>) -> Dynamic {
    let task = Imported::new(task);
    Dynamic::from(host::runtime::Unwitnessed::new(move |witness| {
        Dynamic::from(Plan {
            kind: Kind::Imported(task),
            witness: Some(witness),
        })
    }))
}

/// What makes a tokio future, for [`Native::Tokio`].
#[derive(Clone)]
pub(crate) struct Factory(
    pub(crate) Arc<dyn Fn() -> iced::futures::future::BoxFuture<'static, Dynamic> + Send + Sync>,
);

impl fmt::Debug for Factory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Factory")
    }
}

/// The task of a tokio future `make` makes. A method's task has no
/// witness: its output is a value Rawr code gave it, as `recv` gives what
/// `send` took.
pub(crate) fn tokio_task(
    witness: Option<host::runtime::Witness>,
    make: impl Fn() -> iced::futures::future::BoxFuture<'static, Dynamic> + Send + Sync + 'static,
) -> Dynamic {
    Dynamic::from(Plan {
        kind: Kind::Native(Native::Tokio(Factory(Arc::new(make)))),
        witness,
    })
}

/// tokio's `join!` (or `try_join!`) and `select!` over `values`, each a
/// `Task`.
pub(crate) fn tokio_combined(
    witness: host::runtime::Witness,
    values: &mut [&mut Dynamic],
    join: Option<bool>,
    path: &str,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let plans = values
        .iter_mut()
        .map(|plan| take(plan, path))
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();
    Ok(Dynamic::from(Plan {
        kind: match join {
            Some(fallible) => Kind::Join { plans, fallible },
            None => Kind::Select(plans),
        },
        witness: Some(witness),
    }))
}

/// tokio's `time::timeout`: `plan`'s output in `Ok`, unless `duration`
/// ends first, which gives `Err(elapsed)`.
pub(crate) fn tokio_timeout(
    witness: host::runtime::Witness,
    plan: &mut Dynamic,
    duration: std::time::Duration,
    elapsed: Dynamic,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let plan = take(plan, "time::timeout")?;
    let timer = Plan {
        kind: Kind::Native(Native::Tokio(Factory(Arc::new(move || {
            let elapsed = elapsed.clone();
            let sleep = crate::tokio_facade::sleep(duration);
            Box::pin(async move {
                sleep.await;
                elapsed
            })
        })))),
        witness: None,
    };
    Ok(Dynamic::from(Plan {
        kind: Kind::Select(Box::new([plan, timer])),
        witness: Some(witness),
    }))
}

/// iced's own tasks: `exit`, `window::latest`/`close` and the focus
/// operations. They run in iced's runtime and touch no Rawr authority.
#[derive(Clone, Debug)]
pub(crate) enum Native {
    /// A tokio future, made each time the task runs (`tokio_facade`).
    Tokio(Factory),
    Exit,
    LatestWindow,
    ScaleFactor(iced::window::Id),
    CloseWindow(iced::window::Id),
    FocusNext,
    FocusPrevious,
    FontList,
    Focus(iced::widget::Id),
    Find(iced::widget::Id),
    SnapTo(
        iced::widget::Id,
        iced::widget::scrollable::RelativeOffset,
        iced::widget::operation::Animation,
    ),
    SnapToEnd(iced::widget::Id, iced::widget::operation::Animation),
    ScrollTo(
        iced::widget::Id,
        iced::widget::scrollable::AbsoluteOffset,
        iced::widget::operation::Animation,
    ),
    ScrollBy(
        iced::widget::Id,
        iced::widget::scrollable::AbsoluteOffset,
        iced::widget::operation::Animation,
    ),
}

impl Native {
    fn lower(self) -> iced::Task<Result<Dynamic, Error>> {
        match self {
            Self::Tokio(factory) => iced::Task::future((factory.0)()).map(Ok),
            Self::Exit => iced::exit(),
            Self::LatestWindow => {
                iced::window::latest().map(|id| Ok(rawr::option::from(id)))
            }
            Self::ScaleFactor(id) => {
                iced::window::scale_factor(id).map(|factor| Ok(Dynamic::from(f64::from(factor))))
            }
            Self::CloseWindow(id) => iced::window::close(id),
            Self::FocusNext => iced::widget::operation::focus_next(),
            Self::FontList => iced::font::list().map(|families| {
                Ok(match families {
                    Ok(families) => rawr::result::ok(Dynamic::from_array(
                        families.into_iter().map(Dynamic::from).collect(),
                    )),
                    Err(error) => rawr::result::err(Dynamic::from(error)),
                })
            }),
            Self::FocusPrevious => iced::widget::operation::focus_previous(),
            Self::Focus(id) => iced::widget::operation::focus(id),
            Self::Find(id) => {
                iced::widget::selector::find(id).map(|target| Ok(rawr::option::from(target)))
            }
            Self::SnapTo(id, offset, animation) => {
                iced::widget::operation::scrollable::snap_to(id, offset, animation)
            }
            Self::SnapToEnd(id, animation) => {
                iced::widget::operation::scrollable::snap_to_end(id, animation)
            }
            Self::ScrollTo(id, offset, animation) => {
                iced::widget::operation::scrollable::scroll_to(id, offset, animation)
            }
            Self::ScrollBy(id, offset, animation) => {
                iced::widget::operation::scrollable::scroll_by(id, offset, animation)
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Kind {
    None,
    Done(Dynamic),
    Perform {
        call: Box<host::operation::Call>,
        mapper: item::Value,
    },
    Batch(Box<[Plan]>),
    Chain {
        first: Box<Plan>,
        second: Box<Plan>,
    },
    Map {
        plan: Box<Plan>,
        mapper: item::Value,
    },
    Native(Native),
    /// iced's `Task<Option<T>>::and_then`: `mapper` of each `Some`, a plan.
    AndThen {
        plan: Box<Plan>,
        mapper: item::Value,
    },
    /// An iced `Task` a Rust function returned, actions and all.
    Imported(Imported),
    /// iced's `Task::discard`: the plan runs, and its outputs go nowhere.
    Discard(Box<Plan>),
    /// iced's `Task::sip`: a Rust sipper's progress and output, each mapped.
    Sip {
        sipper: Sip,
        on_progress: item::Value,
        on_output: item::Value,
    },
    /// iced's `Task::abortable`: the plan, abortable through its `Handle`.
    Abortable {
        plan: Box<Plan>,
        abort: Arc<Abort>,
    },
    /// An async body, not yet started: its step function
    /// (`rawr::future`). Each step runs on the UI thread.
    Script(item::Value),
    /// `sender.send(value)`: the value goes to the sipper's progress, and
    /// the task gives `()`.
    Send { sender: Sender, value: Dynamic },
    /// `futures::future::join` (or `try_join`, which ends at the first
    /// `Err`): the plans run at once, and the task gives their outputs.
    Join { plans: Box<[Plan]>, fallible: bool },
    /// tokio's `select!` and `timeout`: the plans run at once, and the
    /// first output ends the others. Plan `i` of `n` gives `Err` `i` times
    /// around its output, and then `Ok` unless it is the last.
    Select(Box<[Plan]>),
}

/// Current executable authority used only while a symbolic plan is lowered.
#[derive(Clone)]
pub(crate) enum Authority {
    Application(Box<rawr::reload::installation::installed::Application>),
    Preview {
        catalog: crate::preview::catalog::Authority,
        key: preview::Key,
    },
}

/// A durable task plan could not be decoded or lowered.
#[derive(Debug)]
pub enum Error {
    /// A runtime value was not a durable task plan.
    Representation {
        /// Actual runtime type name.
        actual: String,
    },
    /// A bound host operation could not issue or complete exactly.
    Operation(rawr::host::operation::Error),
    /// A symbolic task mapper failed under the current application authority.
    Mapper(program::Error),
    /// A symbolic task mapper failed under the selected preview authority.
    PreviewMapper(preview::RuntimeError),
    /// One task-plan node produced a value outside its exact output witness.
    Output(host::runtime::ValueError),
    /// A completed task plan did not produce the application's exact Message.
    Message(application::RuntimeError),
    /// A completed task plan did not produce the selected preview's exact Message.
    PreviewMessage(preview::RuntimeError),
    /// A Rust task's output could not become a Rawr value.
    Imported(String),
    /// A task from Rust was already run. Rust's `Task` is not `Clone`.
    Spent,
}

impl Plan {
    /// Describe no work.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            kind: Kind::None,
            witness: None,
        }
    }

    /// Describe one message ready for immediate delivery.
    #[must_use]
    pub fn done(payload: Dynamic) -> Self {
        Self {
            kind: Kind::Done(payload),
            witness: None,
        }
    }

    fn batch(plans: Vec<Self>, witness: host::runtime::Witness) -> Self {
        Self {
            kind: Kind::Batch(plans.into_boxed_slice()),
            witness: Some(witness),
        }
    }

    fn perform(
        call: host::operation::Call,
        mapper: item::Value,
        witness: host::runtime::Witness,
    ) -> Self {
        Self {
            kind: Kind::Perform {
                call: Box::new(call),
                mapper,
            },
            witness: Some(witness),
        }
    }

    fn chain(self, next: Self, witness: host::runtime::Witness) -> Self {
        Self {
            kind: Kind::Chain {
                first: Box::new(self),
                second: Box::new(next),
            },
            witness: Some(witness),
        }
    }

    fn map(self, mapper: item::Value, witness: host::runtime::Witness) -> Self {
        Self {
            kind: Kind::Map {
                plan: Box::new(self),
                mapper,
            },
            witness: Some(witness),
        }
    }

    /// Return whether this plan describes no work.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self.kind, Kind::None)
    }

    /// Borrow the immediate payload when this is a done plan.
    #[must_use]
    pub const fn done_payload(&self) -> Option<&Dynamic> {
        match &self.kind {
            Kind::None
            | Kind::Perform { .. }
            | Kind::Batch(_)
            | Kind::Chain { .. }
            | Kind::Map { .. }
            | Kind::Native(_)
            | Kind::AndThen { .. }
            | Kind::Imported(_)
            | Kind::Discard(_)
            | Kind::Sip { .. }
            | Kind::Abortable { .. }
            | Kind::Script(_)
            | Kind::Send { .. }
            | Kind::Join { .. }
            | Kind::Select(_) => None,
            Kind::Done(payload) => Some(payload),
        }
    }

    pub(crate) fn from_dynamic(value: Dynamic) -> Result<Self, Error> {
        let actual = value.type_name().to_string();
        value
            .try_cast_result::<Self>()
            .map_err(|_| Error::Representation { actual })
    }

    pub(crate) fn lower(
        self,
        instance: application::Instance,
        generation: program::GenerationId,
        authority: &Authority,
    ) -> Result<iced::Task<runtime::Message>, Error> {
        let lowering = Lowering {
            owner: effect::Owner::new(instance, generation),
            authority: authority.clone(),
        };
        let finish = lowering.finish();
        self.lower_with(&lowering, finish)
    }

    pub(crate) fn prepare_application(
        self,
        authority: &rawr::reload::installation::Application<'_>,
    ) -> Result<Prepared, Error> {
        let Self { kind, witness } = self;
        let kind = match kind {
            Kind::None => PreparedKind::None,
            Kind::Done(payload) => PreparedKind::Done(payload),
            Kind::Perform { call, mapper } => PreparedKind::Perform {
                pending: authority
                    .prepare_operation(*call, &mapper)
                    .map_err(Error::Operation)?,
            },
            Kind::Batch(plans) => PreparedKind::Batch(
                plans
                    .into_vec()
                    .into_iter()
                    .map(|plan| plan.prepare_application(authority))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            ),
            Kind::Chain { first, second } => PreparedKind::Chain {
                first: Box::new(first.prepare_application(authority)?),
                second: Box::new(second.prepare_application(authority)?),
            },
            Kind::Map { plan, mapper } => PreparedKind::Map {
                plan: Box::new(plan.prepare_application(authority)?),
                mapper,
            },
            Kind::Native(native) => PreparedKind::Native(native),
            Kind::AndThen { plan, mapper } => PreparedKind::AndThen {
                plan: Box::new(plan.prepare_application(authority)?),
                mapper,
            },
            Kind::Imported(task) => PreparedKind::Imported(task),
            Kind::Discard(plan) => PreparedKind::Discard(Box::new(plan.prepare_application(authority)?)),
            Kind::Sip {
                sipper,
                on_progress,
                on_output,
            } => PreparedKind::Sip {
                sipper,
                on_progress,
                on_output,
            },
            Kind::Abortable { plan, abort } => PreparedKind::Abortable {
                plan: Box::new(plan.prepare_application(authority)?),
                abort,
            },
            Kind::Script(step) => PreparedKind::Script(step),
            Kind::Send { sender, value } => PreparedKind::Send { sender, value },
            Kind::Join { plans, fallible } => PreparedKind::Join {
                plans: plans
                    .into_vec()
                    .into_iter()
                    .map(|plan| plan.prepare_application(authority))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
                fallible,
            },
            Kind::Select(plans) => PreparedKind::Select(
                plans
                    .into_vec()
                    .into_iter()
                    .map(|plan| plan.prepare_application(authority))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            ),
        };
        Ok(Prepared { kind, witness })
    }

    /// Lower this plan so that each output goes to `next` on the UI thread.
    /// Only native work runs in the returned task's stream.
    fn lower_with(self, lowering: &Lowering, next: Next) -> Result<iced::Task<runtime::Message>, Error> {
        let Self { kind, witness } = self;
        let output = witness.as_ref().and_then(|witness| witness.argument_witness(0));
        let next = lowering.validated(witness, next);
        Ok(match kind {
            Kind::None => iced::Task::none(),
            // A ready value is already on the UI thread.
            Kind::Done(payload) => next(payload),
            Kind::Perform { call, mapper } => {
                let pending = lowering
                    .authority
                    .issue(*call, &mapper)
                    .map_err(Error::Operation)?;
                lowering.operation(pending, next)
            }
            Kind::Batch(plans) => iced::Task::batch(
                plans
                    .into_vec()
                    .into_iter()
                    .map(|plan| plan.lower_with(lowering, next.clone()))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Kind::Chain { first, second } => {
                let first = first.lower_with(lowering, next.clone())?;
                first.chain(second.lower_with(lowering, next)?)
            }
            Kind::Map { plan, mapper } => plan.lower_with(lowering, lowering.mapped(mapper, next))?,
            Kind::Native(native) => lowering.resume(native.lower(), next),
            Kind::AndThen { plan, mapper } => {
                let lowering_inner = lowering.clone();
                let next = Arc::new(move |payload| lowering_inner.and_then(payload, &mapper, next.clone()));
                plan.lower_with(lowering, next)?
            }
            Kind::Imported(task) => lowering.imported(&task, output, next),
            Kind::Discard(plan) => plan.lower_with(lowering, discard())?,
            Kind::Sip {
                sipper,
                on_progress,
                on_output,
            } => lowering.sip(&sipper, on_progress, on_output, next),
            Kind::Abortable { plan, abort } => abort.attach(plan.lower_with(lowering, next)?),
            Kind::Script(step) => lowering.script(step, Dynamic::UNIT, next),
            Kind::Send { sender, value } => lowering.send(&sender, value, next),
            Kind::Join { plans, fallible } => {
                let joined = Joined::new(plans.len(), fallible, next);
                iced::Task::batch(
                    plans
                        .into_vec()
                        .into_iter()
                        .enumerate()
                        .map(|(index, plan)| plan.lower_with(lowering, joined.slot(index)))
                        .collect::<Result<Vec<_>, _>>()?,
                )
            }
            Kind::Select(plans) => {
                let raced = Raced::new(plans.len(), next);
                let mut tasks = Vec::with_capacity(plans.len());
                for (index, plan) in plans.into_vec().into_iter().enumerate() {
                    tasks.push(raced.run(index, plan.lower_with(lowering, raced.slot(index))?));
                }
                iced::Task::batch(tasks)
            }
        })
    }
}

/// The outputs of a join so far. The first output of each plan fills its
/// slot; once every slot is full, or a `try_join` plan gives an `Err`, the
/// join gives its one output.
#[derive(Clone)]
struct Joined(Arc<Mutex<(Vec<Option<Dynamic>>, Option<Next>)>>, bool);

impl Joined {
    fn new(count: usize, fallible: bool, next: Next) -> Self {
        Self(Arc::new(Mutex::new((vec![None; count], Some(next)))), fallible)
    }

    fn slot(&self, index: usize) -> Next {
        let joined = self.clone();
        Arc::new(move |mut output| {
            let mut state = joined.0.lock().unwrap_or_else(PoisonError::into_inner);
            let (slots, next) = &mut *state;
            if next.is_none() || slots[index].is_some() {
                return iced::Task::none();
            }
            if joined.1 {
                match rawr::result::take(&mut output) {
                    Some(Ok(value)) => output = value,
                    Some(Err(error)) => {
                        let next = next.take().expect("the join has not ended");
                        drop(state);
                        return next(rawr::result::err(error));
                    }
                    None => return iced::Task::none(),
                }
            }
            slots[index] = Some(output);
            if slots.iter().any(Option::is_none) {
                return iced::Task::none();
            }
            let outputs = slots.iter_mut().filter_map(Option::take).collect();
            let next = next.take().expect("the join has not ended");
            drop(state);
            let outputs = Dynamic::from_array(outputs);
            next(if joined.1 { rawr::result::ok(outputs) } else { outputs })
        })
    }
}

/// A select's plans while they run. The first output wins: it goes on in
/// the shape [`Kind::Select`] gives, and every other plan is aborted.
#[derive(Clone)]
struct Raced(Arc<Mutex<Race>>);

struct Race {
    next: Option<Next>,
    winner: Option<usize>,
    handles: Vec<Option<iced::task::Handle>>,
}

impl Raced {
    fn new(count: usize, next: Next) -> Self {
        Self(Arc::new(Mutex::new(Race {
            next: Some(next),
            winner: None,
            handles: vec![None; count],
        })))
    }

    /// Plan `index`'s task, abortable by a win of another plan. A plan
    /// lowered after another won (one whose output was ready) does not
    /// run, and a plan that won as it was lowered goes on.
    fn run(&self, index: usize, task: iced::Task<runtime::Message>) -> iced::Task<runtime::Message> {
        let mut race = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match race.winner {
            Some(winner) if winner == index => task,
            Some(_) => iced::Task::none(),
            None => {
                let (task, handle) = task.abortable();
                race.handles[index] = Some(handle);
                task
            }
        }
    }

    fn slot(&self, index: usize) -> Next {
        let raced = self.clone();
        Arc::new(move |output| {
            let mut race = raced.0.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(next) = race.next.take() else {
                return iced::Task::none();
            };
            race.winner = Some(index);
            let count = race.handles.len();
            for (other, handle) in race.handles.iter_mut().enumerate() {
                if other != index
                    && let Some(handle) = handle.take()
                {
                    handle.abort();
                }
            }
            drop(race);
            let mut output = if index + 1 < count {
                rawr::result::ok(output)
            } else {
                output
            };
            for _ in 0..index {
                output = rawr::result::err(output);
            }
            next(output)
        })
    }
}

/// The continuation of a discarded task: nothing.
fn discard() -> Next {
    Arc::new(|_| iced::Task::none())
}

impl Lowering {
    /// The last step: check the application's Message and stamp it.
    fn finish(&self) -> Next {
        let lowering = self.clone();
        Arc::new(move |payload| match lowering.authority.finish(payload) {
            Ok(payload) => iced::Task::done(runtime::Message::new(
                lowering.owner.instance(),
                lowering.owner.generation(),
                payload,
            )),
            Err(error) => lowering.fail(error),
        })
    }

    fn fail(&self, error: Error) -> iced::Task<runtime::Message> {
        iced::Task::done(runtime::Message::task_error(
            self.owner.instance(),
            self.owner.generation(),
            error,
        ))
    }

    /// Check each output against the plan node's witness before `next`.
    fn validated(&self, witness: Option<host::runtime::Witness>, next: Next) -> Next {
        let Some(witness) = witness else {
            return next;
        };
        let lowering = self.clone();
        Arc::new(move |payload| match lowering.authority.validate_output(&witness, payload) {
            Ok(payload) => next(payload),
            Err(error) => lowering.fail(error),
        })
    }

    /// Run the script mapper on each output before `next`.
    fn mapped(&self, mapper: item::Value, next: Next) -> Next {
        let lowering = self.clone();
        Arc::new(move |payload| match lowering.authority.map(payload, &mapper) {
            Ok(payload) => next(payload),
            Err(error) => lowering.fail(error),
        })
    }

    /// iced's `Task<Option<T>>::and_then`: each `Some` given to `mapper`,
    /// whose plan is lowered here, on the UI thread, and then runs.
    fn and_then(
        &self,
        mut payload: Dynamic,
        mapper: &item::Value,
        next: Next,
    ) -> iced::Task<runtime::Message> {
        let actual = payload.type_name().to_string();
        let inner = match rawr::option::take(&mut payload) {
            Some(Some(inner)) => inner,
            Some(None) => return iced::Task::none(),
            None => return self.fail(Error::Representation { actual }),
        };
        let plan = self
            .authority
            .map(inner, mapper)
            .and_then(Plan::from_dynamic)
            .and_then(|plan| plan.lower_with(self, next));
        plan.unwrap_or_else(|error| self.fail(error))
    }

    /// Hand each output of a native task to `next` on the UI thread.
    fn resume(&self, task: iced::Task<Result<Dynamic, Error>>, next: Next) -> iced::Task<runtime::Message> {
        let lowering = self.clone();
        task.then(move |result| {
            let next = next.clone();
            let failed = lowering.clone();
            dispatch::on_ui(lowering.owner, move || match result {
                Ok(payload) => next(payload),
                Err(error) => failed.fail(error),
            })
        })
    }

    /// Run a Rust task's stream natively, actions and all. Each value it
    /// yields becomes a Rawr value, under the output's witness, on the UI
    /// thread before `next`.
    fn imported(
        &self,
        task: &Imported,
        output: Option<host::runtime::Witness>,
        next: Next,
    ) -> iced::Task<runtime::Message> {
        let Some(task) = task.take() else {
            return self.fail(Error::Spent);
        };
        let lowering = self.clone();
        let owner = self.owner;
        dispatch::each_on_ui(owner, task, move |materialize: Materialize| {
            match materialize(output.as_ref()) {
                Ok(payload) => next(payload),
                Err(error) => lowering.fail(Error::Imported(error)),
            }
        })
    }

    /// iced's `Task::sip`: run a Rust sipper natively. Each progress value and
    /// its output become Rawr values on the UI thread, where their mappers
    /// run before `next`.
    fn sip(
        &self,
        sipper: &Sip,
        on_progress: item::Value,
        on_output: item::Value,
        next: Next,
    ) -> iced::Task<runtime::Message> {
        if let Some(builder) = &sipper.script {
            return self.sip_script(builder, on_progress, on_output, next);
        }
        let Some(task) = sipper.take() else {
            return self.fail(Error::Spent);
        };
        let witnesses = (sipper.witness(1), sipper.witness(0));
        let mappers = (on_progress, on_output);
        let lowering = self.clone();
        let owner = self.owner;
        dispatch::each_on_ui(owner, task, move |step: Step| {
            let (materialize, witness, mapper) = match step {
                Step::Progress(materialize) => (materialize, &witnesses.0, &mappers.0),
                Step::Output(materialize) => (materialize, &witnesses.1, &mappers.1),
            };
            let payload = materialize(witness.as_ref())
                .map_err(Error::Imported)
                .and_then(|payload| lowering.authority.map(payload, mapper));
            match payload {
                Ok(payload) => next(payload),
                Err(error) => lowering.fail(error),
            }
        })
    }

    /// iced's `Task::sip` over a script sipper: its builder runs with a
    /// `Sender` whose values go through `on_progress` to `next`, and the
    /// output of the task it returns goes through `on_output`.
    fn sip_script(
        &self,
        builder: &item::Value,
        on_progress: item::Value,
        on_output: item::Value,
        next: Next,
    ) -> iced::Task<runtime::Message> {
        let progress = self.mapped(on_progress, next.clone());
        let sender = Sender(progress);
        let plan = self
            .authority
            .map(Dynamic::from(sender), builder)
            .and_then(Plan::from_dynamic)
            .and_then(|plan| plan.lower_with(self, self.mapped(on_output, next)));
        plan.unwrap_or_else(|error| self.fail(error))
    }

    /// `sender.send(value)`: the progress first, then the sending body
    /// goes on, from a step the UI thread runs, so a loop of sends never
    /// nests.
    fn send(&self, sender: &Sender, value: Dynamic, next: Next) -> iced::Task<runtime::Message> {
        let progress = (sender.0)(value);
        progress.chain(dispatch::on_ui(self.owner, move || next(Dynamic::UNIT)))
    }

    /// Run an async body's next step here, on the UI thread: from its
    /// start with `()`, or from a frame. When it awaits, the awaited task
    /// runs, and its first output resumes the body, again on the UI thread,
    /// after the freshness check of the step that hands it back. A stale
    /// step is dropped, which ends the whole task: a frame never resumes
    /// against new code. When the body ends, its output goes to `next`.
    fn script(&self, step: item::Value, resume: Dynamic, next: Next) -> iced::Task<runtime::Message> {
        let poll = match self.authority.map(resume, &step) {
            Ok(poll) => poll,
            Err(error) => return self.fail(error),
        };
        match poll.try_cast_result::<rawr::future::Poll>() {
            Ok(rawr::future::Poll::Ready(output)) => next(output),
            // A body that never awaits is no machine: it gives its output.
            Err(output) => next(output),
            Ok(rawr::future::Poll::Pending { future, frame }) => {
                let awaited = match Plan::from_dynamic(future) {
                    Ok(awaited) => awaited,
                    Err(error) => return self.fail(error),
                };
                let lowering = self.clone();
                // A future gives one output; a task's later outputs resume
                // nothing.
                let frame = Arc::new(Mutex::new(Some(frame)));
                let resume: Next = Arc::new(move |output| {
                    let Some(frame) = frame.lock().unwrap_or_else(PoisonError::into_inner).take() else {
                        return iced::Task::none();
                    };
                    lowering.script(step.clone(), frame.resume(output), next.clone())
                });
                awaited
                    .lower_with(self, resume)
                    .unwrap_or_else(|error| self.fail(error))
            }
        }
    }

    /// Await an issued operation natively, then validate its output and run
    /// its mapper on the UI thread.
    fn operation(&self, pending: host::operation::Pending, next: Next) -> iced::Task<runtime::Message> {
        let lowering = self.clone();
        iced::Task::future(pending).then(move |completed| {
            let next = next.clone();
            let failed = lowering.clone();
            dispatch::on_ui(lowering.owner, move || {
                match completed.and_then(host::operation::Finish::finish) {
                    Ok(payload) => next(payload),
                    Err(error) => failed.fail(Error::Operation(error)),
                }
            })
        })
    }
}

impl Prepared {
    pub(crate) const fn is_none(&self) -> bool {
        matches!(self.kind, PreparedKind::None)
    }

    pub(crate) fn lower(
        self,
        instance: application::Instance,
        generation: program::GenerationId,
        context: &rawr::reload::installation::installed::Context<'_>,
        authority: Authority,
    ) -> iced::Task<runtime::Message> {
        let lowering = Lowering {
            owner: effect::Owner::new(instance, generation),
            authority,
        };
        let finish = lowering.finish();
        self.lower_with(context, &lowering, finish)
    }

    fn lower_with(
        self,
        context: &rawr::reload::installation::installed::Context<'_>,
        lowering: &Lowering,
        next: Next,
    ) -> iced::Task<runtime::Message> {
        let Self { kind, witness } = self;
        let output = witness.as_ref().and_then(|witness| witness.argument_witness(0));
        let next = lowering.validated(witness, next);
        match kind {
            PreparedKind::None => iced::Task::none(),
            PreparedKind::Done(payload) => next(payload),
            PreparedKind::Perform { pending } => lowering.operation(context.operation(pending), next),
            PreparedKind::Batch(plans) => iced::Task::batch(
                plans
                    .into_vec()
                    .into_iter()
                    .map(|plan| plan.lower_with(context, lowering, next.clone())),
            ),
            PreparedKind::Chain { first, second } => first
                .lower_with(context, lowering, next.clone())
                .chain(second.lower_with(context, lowering, next)),
            PreparedKind::Map { plan, mapper } => {
                plan.lower_with(context, lowering, lowering.mapped(mapper, next))
            }
            PreparedKind::Native(native) => lowering.resume(native.lower(), next),
            PreparedKind::AndThen { plan, mapper } => {
                let lowering_inner = lowering.clone();
                let next = Arc::new(move |payload| lowering_inner.and_then(payload, &mapper, next.clone()));
                plan.lower_with(context, lowering, next)
            }
            PreparedKind::Imported(task) => lowering.imported(&task, output, next),
            PreparedKind::Discard(plan) => plan.lower_with(context, lowering, discard()),
            PreparedKind::Sip {
                sipper,
                on_progress,
                on_output,
            } => lowering.sip(&sipper, on_progress, on_output, next),
            PreparedKind::Abortable { plan, abort } => {
                abort.attach(plan.lower_with(context, lowering, next))
            }
            PreparedKind::Script(step) => lowering.script(step, Dynamic::UNIT, next),
            PreparedKind::Send { sender, value } => lowering.send(&sender, value, next),
            PreparedKind::Join { plans, fallible } => {
                let joined = Joined::new(plans.len(), fallible, next);
                iced::Task::batch(
                    plans
                        .into_vec()
                        .into_iter()
                        .enumerate()
                        .map(|(index, plan)| plan.lower_with(context, lowering, joined.slot(index))),
                )
            }
            PreparedKind::Select(plans) => {
                let raced = Raced::new(plans.len(), next);
                let tasks = plans
                    .into_vec()
                    .into_iter()
                    .enumerate()
                    .map(|(index, plan)| {
                        raced.run(index, plan.lower_with(context, lowering, raced.slot(index)))
                    })
                    .collect::<Vec<_>>();
                iced::Task::batch(tasks)
            }
        }
    }
}

impl Authority {
    fn map(&self, payload: Dynamic, mapper: &item::Value) -> Result<Dynamic, Error> {
        match self {
            Self::Application(launch) => {
                launch.invoke(mapper, &mut [payload]).map_err(Error::Mapper)
            }
            Self::Preview { catalog, key } => catalog
                .map_task_value(key, mapper, payload)
                .map_err(Error::PreviewMapper),
        }
    }

    fn validate_output(
        &self,
        witness: &host::runtime::Witness,
        payload: Dynamic,
    ) -> Result<Dynamic, Error> {
        match self {
            Self::Application(launch) => launch.validate_witness_argument(witness, 0, &payload),
            Self::Preview { catalog, .. } => {
                catalog.validate_witness_argument(witness, 0, &payload)
            }
        }
        .map_err(Error::Output)?;
        Ok(payload)
    }

    fn finish(&self, payload: Dynamic) -> Result<Dynamic, Error> {
        match self {
            Self::Application(launch) => {
                launch.validate_message(&payload).map_err(Error::Message)?
            }
            Self::Preview { catalog, key } => catalog
                .validate_message(key, &payload)
                .map_err(Error::PreviewMessage)?,
        }
        Ok(payload)
    }
}

impl Authority {
    pub(crate) fn application(launch: rawr::reload::installation::installed::Application) -> Self {
        Self::Application(Box::new(launch))
    }

    pub(crate) fn preview(catalog: crate::preview::catalog::Authority, key: preview::Key) -> Self {
        Self::Preview { catalog, key }
    }

    fn issue(
        &self,
        call: host::operation::Call,
        mapper: &item::Value,
    ) -> Result<host::operation::Pending, host::operation::Error> {
        match self {
            Self::Application(launch) => launch.issue_operation(call, mapper),
            Self::Preview { catalog, key } => catalog.issue_operation(key, call, mapper),
        }
    }
}

impl Default for Plan {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
impl Plan {
    /// A Rust task whose outputs go nowhere, as `Task::discard` of one.
    pub(crate) fn discarded_rust_task(task: iced::Task<Materialize>) -> Self {
        Self {
            kind: Kind::Discard(Box::new(Self {
                kind: Kind::Imported(Imported::new(task)),
                witness: None,
            })),
            witness: None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Representation { actual } => write!(
                formatter,
                "application callback returned `{actual}`, expected a durable iced::Task plan"
            ),
            Self::Operation(error) => error.fmt(formatter),
            Self::Mapper(error) => error.fmt(formatter),
            Self::PreviewMapper(error) => error.fmt(formatter),
            Self::Output(error) => write!(formatter, "task plan produced invalid output: {error}"),
            Self::Message(error) => {
                write!(formatter, "task plan produced invalid Message: {error}")
            }
            Self::PreviewMessage(error) => {
                write!(
                    formatter,
                    "preview task plan produced invalid Message: {error}"
                )
            }
            Self::Imported(error) => {
                write!(formatter, "a Rust task's output could not cross into Rawr: {error}")
            }
            Self::Spent => formatter.write_str("this task from Rust already ran, and a Task runs once"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Representation { .. } => None,
            Self::Operation(error) => Some(error),
            Self::Mapper(error) => Some(error),
            Self::PreviewMapper(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Message(error) => Some(error),
            Self::PreviewMessage(error) => Some(error),
            Self::Imported(_) | Self::Spent => None,
        }
    }
}

pub(crate) fn bind(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    let builder = iced;
    for (path, native) in [
        ("iced::exit", Native::Exit),
        ("iced::window::latest", Native::LatestWindow),
        ("iced::widget::operation::focus_next", Native::FocusNext),
        ("iced::widget::operation::focus_previous", Native::FocusPrevious),
        ("iced::font::list", Native::FontList),
    ] {
        builder
            .bind_with_context(path, move |call, args| {
                if !args.is_empty() {
                    return Err(arity(path, 0, args.len()));
                }
                Ok(Dynamic::from(Plan {
                    kind: Kind::Native(native.clone()),
                    witness: Some(call.output_witness().map_err(witness_error)?),
                }))
            })
            .expect("the iced facade must retain every native task");
    }
    builder
        .bind_with_context("iced::Task::batch", |call, args| {
            let [plans] = args else {
                return Err(arity("Task::batch", 1, args.len()));
            };
            let plans = plans.take().into_array().map_err(|actual| {
                EvalAltResult::ErrorRuntime(
                    format!("Task::batch expected a list, found `{actual}`").into(),
                    Position::NONE,
                )
            })?;
            let plans = plans
                .into_iter()
                .map(|mut plan| take(&mut plan, "Task::batch"))
                .collect::<Result<Vec<_>, _>>()?;
            let witness = call.output_witness().map_err(witness_error)?;
            Ok(Dynamic::from(Plan::batch(plans, witness)))
        })
        .expect("the iced facade must retain Task::batch");
    builder
        .bind_with_context("iced::widget::operation::focus", |call, args| {
            let [id] = args else {
                return Err(arity("operation::focus", 1, args.len()));
            };
            native_task(call, Native::Focus(take_id(id)?))
        })
        .expect("the iced facade must retain operation::focus");
    builder
        .bind_with_context("iced::widget::selector::find", |call, args| {
            let [id] = args else {
                return Err(arity("selector::find", 1, args.len()));
            };
            native_task(call, Native::Find(take_id(id)?))
        })
        .expect("the iced facade must retain selector::find");
    for name in ["bounds", "visible_bounds"] {
        let path = format!("iced::widget::selector::Target::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [target] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                let target = target
                    .read_lock::<iced::widget::selector::Target>()
                    .ok_or_else(|| {
                        EvalAltResult::ErrorRuntime(
                            format!("{path} expects a `widget::selector::Target`").into(),
                            Position::NONE,
                        )
                    })?;
                Ok(match name {
                    "bounds" => Dynamic::from(target.bounds()),
                    _ => rawr::option::from(target.visible_bounds()),
                })
            })
            .expect("the iced facade must retain every selector::Target reader");
    }
    for name in ["snap_to", "snap_to_end", "scroll_to", "scroll_by"] {
        let path = format!("iced::widget::operation::scrollable::{name}");
        builder
            .bind_with_context(path.clone(), move |call, args| {
                let expected = if name == "snap_to_end" { 2 } else { 3 };
                if args.len() != expected {
                    return Err(arity(&path, expected, args.len()));
                }
                let id = take_id(args[0])?;
                let animation = take_value::<iced::widget::operation::Animation>(
                    args[expected - 1],
                    "operation::Animation",
                )?;
                let native = match name {
                    "snap_to" => Native::SnapTo(
                        id,
                        take_value(args[1], "scrollable::RelativeOffset")?,
                        animation,
                    ),
                    "snap_to_end" => Native::SnapToEnd(id, animation),
                    "scroll_to" => Native::ScrollTo(
                        id,
                        take_value(args[1], "scrollable::AbsoluteOffset")?,
                        animation,
                    ),
                    _ => Native::ScrollBy(
                        id,
                        take_value(args[1], "scrollable::AbsoluteOffset")?,
                        animation,
                    ),
                };
                native_task(call, native)
            })
            .expect("the iced facade must retain every scrollable operation");
    }
    builder
        .bind_with_context("iced::window::scale_factor", |call, args| {
            let [id] = args else {
                return Err(arity("window::scale_factor", 1, args.len()));
            };
            native_task(call, Native::ScaleFactor(take_value(id, "window::Id")?))
        })
        .expect("the iced facade must retain window::scale_factor");
    builder
        .bind_with_context("iced::window::close", |call, args| {
            if args.len() != 1 {
                return Err(arity("window::close", 1, args.len()));
            }
            let id = args[0]
                .take()
                .try_cast_result::<iced::window::Id>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!("window::close received `{}`, expected a `window::Id`", value.type_name())
                            .into(),
                        Position::NONE,
                    )
                })?;
            Ok(Dynamic::from(Plan {
                kind: Kind::Native(Native::CloseWindow(id)),
                witness: Some(call.output_witness().map_err(witness_error)?),
            }))
        })
        .expect("the iced facade must retain window::close");
    builder
        .bind_with_context("iced::Task::and_then", |call, args| {
            if args.len() != 2 {
                return Err(arity("Task::and_then", 2, args.len()));
            }
            let plan = take(args[0], "Task::and_then")?;
            let witness = plan.witness.as_ref().ok_or_else(|| {
                EvalAltResult::ErrorRuntime(
                    "Task::and_then received an unattested plan".into(),
                    Position::NONE,
                )
            })?;
            let witness = call.transform_witness(witness).map_err(witness_error)?;
            let mapper = args[1]
                .take()
                .try_cast_result::<item::Value>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!(
                            "Task::and_then received `{}`, expected an exact symbolic function",
                            value.type_name()
                        )
                        .into(),
                        Position::NONE,
                    )
                })?;
            Ok(Dynamic::from(Plan {
                kind: Kind::AndThen {
                    plan: Box::new(plan),
                    mapper,
                },
                witness: Some(witness),
            }))
        })
        .expect("the iced facade must retain Task::and_then");
    builder
        .bind_type_attestation("iced::Task", |value, expected| {
            let plan = value.read_lock::<Plan>()?;
            // `sender.send(value)` is a method, which mints no witness. Its
            // task gives `()` and nothing else.
            if matches!(plan.kind, Kind::Send { .. }) {
                return expected.attest(
                    expected.argument_count() == 1 && expected.argument_matches(0, &Dynamic::UNIT),
                );
            }
            // A tokio method's task gives a value Rawr code gave it.
            if plan.witness.is_none() && matches!(plan.kind, Kind::Native(Native::Tokio(_))) {
                return expected.attest(expected.argument_count() == 1);
            }
            if expected.argument_count() != 1
                || !plan
                    .done_payload()
                    .is_none_or(|payload| expected.argument_matches(0, payload))
            {
                return None;
            }
            expected.attest_witness(plan.witness.as_ref())
        })
        .expect("the iced facade must declare generic Task exactly once");
    builder
        .bind_with_context("iced::Task::none", |call, args| {
            if !args.is_empty() {
                return Err(arity("Task::none", 0, args.len()));
            }
            Ok(Dynamic::from(Plan {
                kind: Kind::None,
                witness: Some(call.output_witness().map_err(witness_error)?),
            }))
        })
        .expect("the iced facade must retain the exact Task::none item");
    builder
        .bind_with_context("iced::Task::done", |call, args| {
            if args.len() != 1 {
                return Err(arity("Task::done", 1, args.len()));
            }
            Ok(Dynamic::from(Plan {
                kind: Kind::Done(args[0].take()),
                witness: Some(call.output_witness().map_err(witness_error)?),
            }))
        })
        .expect("the iced facade must retain the exact Task::done item");
    builder
        .bind_with_context("iced::Task::perform", |call, args| {
            if args.len() != 2 {
                return Err(arity("Task::perform", 2, args.len()));
            }
            let operation_call = args[0]
                .take()
                .try_cast_result::<host::operation::Call>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!(
                            "Task::perform received `{}`, expected a sealed host operation call",
                            value.type_name()
                        )
                        .into(),
                        Position::NONE,
                    )
                })?;
            let mapper = args[1]
                .take()
                .try_cast_result::<item::Value>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!(
                            "Task::perform received `{}`, expected an exact symbolic mapper",
                            value.type_name()
                        )
                        .into(),
                        Position::NONE,
                    )
                })?;
            let witness = call.output_witness().map_err(witness_error)?;
            Ok(Dynamic::from(Plan::perform(
                operation_call,
                mapper,
                witness,
            )))
        })
        .expect("the iced facade must retain the exact Task::perform item");
    rawr.bind_with_context("rawr_iced::task::script", |call, args| {
        let [step] = args else {
            return Err(arity("task::script", 1, args.len()));
        };
        let step = take_value::<item::Value>(step, "an async body's step function")?;
        Ok(Dynamic::from(Plan {
            kind: Kind::Script(step),
            witness: Some(call.output_witness().map_err(witness_error)?),
        }))
    })
    .expect("the rawr_iced facade must retain the async body's Task");
    rawr.bind_with_context("rawr_iced::task::batch", |call, args| {
        let mut plans = Vec::with_capacity(args.len());
        for value in args {
            plans.push(take(value, "Task::batch")?);
        }
        let witness = call.output_witness().map_err(witness_error)?;
        Ok(Dynamic::from(Plan::batch(plans, witness)))
    })
    .expect("the rawr_iced facade must retain the exact Task list builder");
    builder
        .bind_with_context("iced::Task::chain", |call, args| {
            if args.len() != 2 {
                return Err(arity("Task::chain", 2, args.len()));
            }
            let first = take(args[0], "Task::chain")?;
            let witness = first.witness.as_ref().ok_or_else(|| {
                EvalAltResult::ErrorRuntime(
                    "Task::chain received an unattested plan".into(),
                    Position::NONE,
                )
            })?;
            let witness = call.transform_witness(witness).map_err(witness_error)?;
            let second = take(args[1], "Task::chain")?;
            Ok(Dynamic::from(first.chain(second, witness)))
        })
        .expect("the iced facade must retain the exact Task::chain item");
    builder
        .bind_with_context("iced::Task::future", |_call, args| {
            // A Rust future already crossed as a `Task` of its output.
            let [future] = args else {
                return Err(arity("Task::future", 1, args.len()));
            };
            Ok(Dynamic::from(take(future, "Task::future")?))
        })
        .expect("the iced facade must retain Task::future");
    builder
        .bind_with_context("iced::Task::discard", |call, args| {
            let [plan] = args else {
                return Err(arity("Task::discard", 1, args.len()));
            };
            let plan = take(plan, "Task::discard")?;
            let witness = plan.witness.as_ref().ok_or_else(|| {
                EvalAltResult::ErrorRuntime(
                    "Task::discard received an unattested plan".into(),
                    Position::NONE,
                )
            })?;
            let witness = call.transform_witness(witness).map_err(witness_error)?;
            Ok(Dynamic::from(Plan {
                kind: Kind::Discard(Box::new(plan)),
                witness: Some(witness),
            }))
        })
        .expect("the iced facade must retain Task::discard");
    builder
        .bind_with_context("iced::Task::sip", |call, args| {
            let [sipper, on_progress, on_output] = args else {
                return Err(arity("Task::sip", 3, args.len()));
            };
            let sipper = take_value::<Sip>(sipper, "task::Sipper")?;
            let on_progress = take_value::<item::Value>(on_progress, "a symbolic function")?;
            let on_output = take_value::<item::Value>(on_output, "a symbolic function")?;
            Ok(Dynamic::from(Plan {
                kind: Kind::Sip {
                    sipper,
                    on_progress,
                    on_output,
                },
                witness: Some(call.output_witness().map_err(witness_error)?),
            }))
        })
        .expect("the iced facade must retain Task::sip");
    for (path, fallible) in [
        ("iced::futures::future::join", false),
        ("iced::futures::future::try_join", true),
    ] {
        builder
            .bind_with_context(path, move |call, args| {
                let plans = args
                    .iter_mut()
                    .map(|plan| take(plan, path))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Dynamic::from(Plan {
                    kind: Kind::Join {
                        plans: plans.into_boxed_slice(),
                        fallible,
                    },
                    witness: Some(call.output_witness().map_err(witness_error)?),
                }))
            })
            .expect("the iced facade must retain futures::future::join and try_join");
    }
    builder
        .bind_with_context("iced::task::sipper", |call, args| {
            let [builder] = args else {
                return Err(arity("task::sipper", 1, args.len()));
            };
            let builder = take_value::<item::Value>(builder, "a symbolic function")?;
            Ok(Dynamic::from(Sip {
                task: Arc::new(Mutex::new(None)),
                script: Some(builder),
                witness: Some(call.output_witness().map_err(witness_error)?),
            }))
        })
        .expect("the iced facade must retain task::sipper");
    // A sender carries no type of its own: the `Task::sip` that made it
    // checks each value it sends through the progress mapper.
    builder
        .bind_type_attestation("iced::task::Sender", |value, expected| {
            expected.attest(value.read_lock::<Sender>().is_some())
        })
        .expect("the iced facade must declare task::Sender exactly once");
    builder
        .bind_with_context("iced::task::Sender::send", |_call, args| {
            let [sender, value] = args else {
                return Err(arity("task::Sender::send", 2, args.len()));
            };
            let sender = sender.read_lock::<Sender>().map(|sender| sender.clone()).ok_or_else(|| {
                EvalAltResult::ErrorRuntime("expected a `task::Sender`".into(), Position::NONE)
            })?;
            Ok(Dynamic::from(Plan {
                kind: Kind::Send {
                    sender,
                    value: value.take(),
                },
                // A method mints no witness; the sending body awaits this
                // task at once.
                witness: None,
            }))
        })
        .expect("the iced facade must retain task::Sender::send");
    builder
        .bind_type_attestation("iced::task::Sipper", |value, expected| {
            let sipper = value.read_lock::<Sip>()?;
            expected.attest_witness(sipper.witness.as_ref())
        })
        .expect("the iced facade must declare task::Sipper exactly once");
    builder
        .bind_with_context("iced::Task::abortable", |_call, args| {
            let [plan] = args else {
                return Err(arity("Task::abortable", 1, args.len()));
            };
            let plan = take(plan, "Task::abortable")?;
            // The abortable task has the receiver's own type and witness.
            let witness = plan.witness.clone();
            let abort = Arc::new(Abort::default());
            let handle = Handle {
                abort: abort.clone(),
                guard: None,
            };
            let plan = Plan {
                kind: Kind::Abortable {
                    plan: Box::new(plan),
                    abort,
                },
                witness,
            };
            Ok(Dynamic::from_array(vec![
                Dynamic::from(plan),
                Dynamic::from(handle),
            ]))
        })
        .expect("the iced facade must retain Task::abortable");
    builder
        .bind("iced::task::Handle::abort_on_drop", |args| {
            let [handle] = args else {
                return Err(arity("task::Handle::abort_on_drop", 1, args.len()));
            };
            let handle = take_value::<Handle>(handle, "task::Handle")?;
            let guard = Arc::new(Guard(handle.abort.clone()));
            Ok(Dynamic::from(Handle {
                abort: handle.abort,
                guard: Some(guard),
            }))
        })
        .expect("the iced facade must retain task::Handle::abort_on_drop");
    builder
        .bind("iced::task::Handle::abort", |args| {
            let [handle] = args else {
                return Err(arity("task::Handle::abort", 1, args.len()));
            };
            let handle = handle.read_lock::<Handle>().ok_or_else(|| {
                EvalAltResult::ErrorRuntime("expected a `task::Handle`".into(), Position::NONE)
            })?;
            handle.abort.abort();
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain task::Handle::abort");
    builder
        .bind("iced::task::Handle::is_aborted", |args| {
            let [handle] = args else {
                return Err(arity("task::Handle::is_aborted", 1, args.len()));
            };
            let handle = handle.read_lock::<Handle>().ok_or_else(|| {
                EvalAltResult::ErrorRuntime("expected a `task::Handle`".into(), Position::NONE)
            })?;
            Ok(Dynamic::from(handle.abort.is_aborted()))
        })
        .expect("the iced facade must retain task::Handle::is_aborted");
    builder
        .bind_with_context("iced::Task::map", |call, args| {
            if args.len() != 2 {
                return Err(arity("Task::map", 2, args.len()));
            }
            let plan = take(args[0], "Task::map")?;
            let witness = plan.witness.as_ref().ok_or_else(|| {
                EvalAltResult::ErrorRuntime(
                    "Task::map received an unattested plan".into(),
                    Position::NONE,
                )
            })?;
            let witness = call.transform_witness(witness).map_err(witness_error)?;
            let mapper = args[1]
                .take()
                .try_cast_result::<item::Value>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!(
                            "Task::map received `{}`, expected an exact symbolic mapper",
                            value.type_name()
                        )
                        .into(),
                        Position::NONE,
                    )
                })?;
            Ok(Dynamic::from(plan.map(mapper, witness)))
        })
        .expect("the iced facade must retain the exact Task::map item");
}

fn native_task(
    call: &host::runtime::Call<'_>,
    native: Native,
) -> Result<Dynamic, Box<EvalAltResult>> {
    Ok(Dynamic::from(Plan {
        kind: Kind::Native(native),
        witness: Some(call.output_witness().map_err(witness_error)?),
    }))
}

/// iced's `impl Into<Id>`: an `Id`, or a string.
pub(crate) fn take_id(value: &mut Dynamic) -> Result<iced::widget::Id, Box<EvalAltResult>> {
    if value.is_string() {
        let id = value.take().into_string().unwrap_or_default();
        return Ok(iced::widget::Id::from(id));
    }
    take_value(value, "widget::Id")
}

fn take_value<T: Clone + 'static>(
    value: &mut Dynamic,
    expected: &str,
) -> Result<T, Box<EvalAltResult>> {
    let actual = value.type_name();
    value.take().try_cast_result::<T>().map_err(|_| {
        EvalAltResult::ErrorRuntime(
            format!("native task binding received `{actual}`, expected `{expected}`").into(),
            Position::NONE,
        )
        .into()
    })
}

fn take(value: &mut Dynamic, operation: &str) -> Result<Plan, Box<EvalAltResult>> {
    value.take().try_cast_result::<Plan>().map_err(|value| {
        EvalAltResult::ErrorRuntime(
            format!(
                "{operation} received `{}`, expected a durable iced::Task plan",
                value.type_name()
            )
            .into(),
            Position::NONE,
        )
        .into()
    })
}

fn witness_error(error: host::runtime::WitnessError) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(error.to_string().into(), Position::NONE).into()
}

fn arity(operation: &str, expected: usize, actual: usize) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(
        format!(
            "native task binding `{operation}` received {actual} operand(s), expected {expected}"
        )
        .into(),
        Position::NONE,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::{Error, Kind, Plan};
    use iced_test::futures::futures::StreamExt;
    use rawr::{host, Dynamic, Engine, Shared};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn a_rust_task_crosses_as_itself_and_a_future_through_task_future() {
        use super::{Project, ViaFuture as _, ViaTask as _};

        let task = (&Project::new(iced::Task::done(1_i64))).into_task();
        assert_eq!(outputs(task), [1]);
        let future = (&Project::new(async { 2_i64 })).into_task();
        assert_eq!(outputs(future), [2]);
    }

    #[test]
    fn a_handle_aborts_its_task_before_or_after_lowering() {
        use super::Abort;
        use std::sync::Arc;

        let pending = || iced::Task::future(std::future::pending::<i64>()).map(|_| unreachable!());
        let after = Arc::new(Abort::default());
        let task: iced::Task<crate::runtime::Message> = after.attach(pending());
        after.abort();
        assert!(outputs(task).is_empty());

        let before = Arc::new(Abort::default());
        before.abort();
        let task: iced::Task<crate::runtime::Message> = before.attach(pending());
        assert!(outputs(task).is_empty());
    }

    #[test]
    fn abort_on_drop_aborts_when_its_last_copy_drops() {
        use super::{Abort, Guard, Handle};
        use std::sync::Arc;

        let abort = Arc::new(Abort::default());
        let handle = Handle {
            abort: abort.clone(),
            guard: Some(Arc::new(Guard(abort.clone()))),
        };
        let copy = handle.clone();
        drop(handle);
        assert!(!abort.is_aborted());
        drop(copy);
        assert!(abort.is_aborted());
    }

    fn outputs<T: Send + 'static>(task: iced::Task<T>) -> Vec<T> {
        let Some(stream) = iced_test::runtime::task::into_stream(task) else {
            return Vec::new();
        };
        iced_test::futures::futures::executor::block_on(stream.collect::<Vec<_>>())
            .into_iter()
            .filter_map(|action| match action {
                iced_test::runtime::Action::Output(output) => Some(output),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn plan_round_trips_through_dynamic_without_losing_its_payload() {
        let plan = Plan::done(42_i64.into());
        let dynamic = Dynamic::from(plan.clone());
        let round_trip = Plan::from_dynamic(dynamic).unwrap();

        assert_eq!(plan.done_payload().unwrap().as_int().unwrap(), 42);
        assert_eq!(round_trip.done_payload().unwrap().as_int().unwrap(), 42);
    }

    #[test]
    fn stale_nominal_argument_fails_before_factory_start() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut old = adapter(old_source(), effects.clone(), starts.clone());
        let plan = old.take_startup_plan();
        let current = adapter(
            &current_source(
                "bool",
                "pub fn loaded(value: i64) -> Message { Message::Loaded(value) }",
            ),
            effects,
            starts.clone(),
        );

        let result = plan.lower(
            current.instance(),
            current.generation(),
            &current.task_authority(),
        );

        assert!(matches!(
            result,
            Err(Error::Operation(host::operation::Error::Input {
                index: 1,
                ..
            }))
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn missing_current_mapper_fails_before_factory_start() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut old = adapter(old_source(), effects.clone(), starts.clone());
        let plan = old.take_startup_plan();
        let current = adapter(&current_source("i64", ""), effects, starts.clone());

        let result = plan.lower(
            current.instance(),
            current.generation(),
            &current.task_authority(),
        );

        assert!(matches!(
            result,
            Err(Error::Operation(host::operation::Error::Mapper(_)))
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lowered_work_keeps_its_issuer_while_unlowered_work_rebinds_current_code() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut old = adapter(old_source(), effects.clone(), starts.clone());
        let old_instance = old.instance();
        let old_generation = old.generation();
        let lowered = old
            .take_startup_plan()
            .lower(old_instance, old_generation, &old.task_authority())
            .unwrap();

        let mut symbolic = adapter(old_source(), effects.clone(), starts.clone());
        let symbolic = symbolic.take_startup_plan();
        let mut current = adapter(
            &current_source(
                "i64",
                "pub fn loaded(value: i64) -> Message { Message::Loaded(value + 1) }",
            ),
            effects,
            starts.clone(),
        );
        let current_instance = current.instance();
        let current_generation = current.generation();
        let rebound = symbolic
            .lower(
                current_instance,
                current_generation,
                &current.task_authority(),
            )
            .unwrap();

        let old_message = one_message(lowered);
        let current_message = one_message(rebound);
        assert_eq!(old_message.instance(), old_instance);
        assert_eq!(old_message.generation(), old_generation);
        assert_eq!(current_message.instance(), current_instance);
        assert_eq!(current_message.generation(), current_generation);
        assert_eq!(
            current.update(current_message).unwrap().delivery(),
            crate::program::Delivery::Applied
        );
        iced_test::simulator(current.view()).find("8").unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn lowering_defers_the_operation_factory_until_the_task_is_polled() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut adapter = adapter(old_source(), effects, starts.clone());
        let plan = adapter.take_startup_plan();
        let task = plan
            .lower(
                adapter.instance(),
                adapter.generation(),
                &adapter.task_authority(),
            )
            .unwrap();

        assert_eq!(starts.load(Ordering::SeqCst), 0);
        let _message = one_message(task);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dropping_lowered_work_never_starts_the_operation_factory() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut adapter = adapter(old_source(), effects, starts.clone());
        let plan = adapter.take_startup_plan();
        let task = plan
            .lower(
                adapter.instance(),
                adapter.generation(),
                &adapter.task_authority(),
            )
            .unwrap();

        assert_eq!(starts.load(Ordering::SeqCst), 0);
        drop(task);
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn invalid_intermediate_output_stops_before_the_remaining_mapper_chain() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut adapter = adapter(
            &nested_source("Child::Leaf(message)"),
            effects,
            starts.clone(),
        );
        let mut plan = adapter.take_startup_plan();
        let Kind::Map { plan: child, .. } = &mut plan.kind else {
            panic!("the fixture must retain the outer parent map");
        };
        let Kind::Map { plan: perform, .. } = &mut child.kind else {
            panic!("the fixture must retain the intermediate child map");
        };
        perform.kind = Kind::Done(Dynamic::from(true));

        let task = plan
            .lower(
                adapter.instance(),
                adapter.generation(),
                &adapter.task_authority(),
            )
            .unwrap();
        let failure = one_message(task);

        assert!(matches!(
            failure.task_failure().map(AsRef::as_ref),
            Some(Error::Output(_))
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn invalid_final_output_is_rejected_before_it_becomes_a_runtime_message() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let adapter = adapter(old_source(), effects, starts.clone());
        let task = Plan::done(Dynamic::from(true))
            .lower(
                adapter.instance(),
                adapter.generation(),
                &adapter.task_authority(),
            )
            .unwrap();
        let failure = one_message(task);

        assert!(matches!(
            failure.task_failure().map(AsRef::as_ref),
            Some(Error::Message(_))
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn throwing_intermediate_mapper_becomes_a_task_failure() {
        let effects = effects_facade();
        let starts = Arc::new(AtomicUsize::new(0));
        let mut adapter = adapter(
            &nested_source("throw \"intermediate mapper failed\""),
            effects,
            starts.clone(),
        );
        let plan = adapter.take_startup_plan();
        let task = plan
            .lower(
                adapter.instance(),
                adapter.generation(),
                &adapter.task_authority(),
            )
            .unwrap();
        let failure = one_message(task);

        assert!(matches!(
            failure.task_failure().map(AsRef::as_ref),
            Some(Error::Mapper(error)) if error.to_string().contains("intermediate mapper failed")
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    fn adapter(
        source: &str,
        effects: host::facade::Definition,
        starts: Arc<AtomicUsize>,
    ) -> crate::program::Adapter {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(effects.clone()).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        crate::program::Adapter::boot(launch.bind(registry(effects, starts)).unwrap()).unwrap()
    }

    fn registry(
        effects: host::facade::Definition,
        starts: Arc<AtomicUsize>,
    ) -> host::runtime::Registry {
        let operation = effects
            .items()
            .find(|item| item.path() == "effects::accept")
            .unwrap()
            .clone();
        let mut binding = host::runtime::Builder::new(effects);
        binding
            .bind_operation(
                &operation,
                move |arguments| {
                    starts.fetch_add(1, Ordering::SeqCst);
                    async move {
                        assert_eq!(arguments.len(), 2);
                        Ok::<_, ()>(Dynamic::from(7_i64))
                    }
                },
                |_| Dynamic::from(-1_i64),
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry.install(binding.finish()).unwrap();
        registry
    }

    fn effects_facade() -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
        let mut accept = host::signature::Builder::new();
        accept.type_parameter("First", &[]).unwrap();
        accept.type_parameter("Second", &[]).unwrap();
        accept.parameter(host::type_ref::Type::parameter("First").unwrap());
        accept.parameter(host::type_ref::Type::parameter("Second").unwrap());
        accept.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
        builder
            .item(host::item::Definition::operation("accept", accept.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    fn one_message(task: iced::Task<crate::runtime::Message>) -> crate::runtime::Message {
        let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
        // Script steps come back as resume requests, which the host runs
        // on its own thread; this test thread is that thread.
        iced_test::futures::futures::executor::block_on(async move {
            loop {
                match stream.next().await {
                    Some(iced_test::runtime::Action::Output(message)) => {
                        match message.resumption() {
                            Some(resume) => {
                                if let Some(message) = resume.run() {
                                    return message;
                                }
                            }
                            None => return message,
                        }
                    }
                    Some(action) => panic!("expected message output, found {action:?}"),
                    None => panic!("operation task must emit one message"),
                }
            }
        })
    }

    fn old_source() -> &'static str {
        r#"
use effects::accept;
use iced::Task;
use iced::Element;
use iced::widget::text;
struct Payload { value: i64 }
struct State;
#[derive(Debug, Clone, Copy)] enum Message { Loaded(i64) }
pub fn loaded(value: i64) -> Message { Message::Loaded(value) }
impl State {
    fn boot() -> (State, Task<Message>) {
        (State {}, Task::perform(accept(1, Payload { value: 1 }), loaded))
    }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("old") }
}
pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
    }

    fn current_source(payload: &str, mapper: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::text;
struct Payload {{ value: {payload} }}
#[derive(Default)] struct State {{ value: i64 }}
#[derive(Debug, Clone, Copy)] enum Message {{ Loaded(i64) }}
{mapper}
impl State {{
    fn update(&mut self, message: Message) {{
        match message {{ Message::Loaded(value) => self.value = value }}
    }}
    fn view(&self) -> Element<'_, Message> {{ text(self.value) }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }

    fn nested_source(child: &str) -> String {
        format!(
            r#"
use effects::accept;
use iced::Task;
use iced::Element;
use iced::widget::text;
struct Payload {{ value: i64 }}
struct State;
#[derive(Debug, Clone, Copy)] enum Leaf {{ Loaded(i64) }}
#[derive(Debug, Clone, Copy)] enum Child {{ Leaf(Leaf) }}
#[derive(Debug, Clone, Copy)] enum Message {{ Child(Child) }}
pub fn leaf(value: i64) -> Leaf {{ Leaf::Loaded(value) }}
pub fn child(message: Leaf) -> Child {{ {child} }}
pub fn parent(message: Child) -> Message {{ Message::Child(message) }}
impl State {{
    fn boot() -> (State, Task<Message>) {{
        let task = Task::perform(accept(1, Payload {{ value: 1 }}), leaf)
            .map(child)
            .map(parent);
        (State {{}}, task)
    }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ text("nested") }}
}}
pub fn main() -> iced::Result {{
    iced::application(State::boot, State::update, State::view).run()
}}
"#
        )
    }
}
