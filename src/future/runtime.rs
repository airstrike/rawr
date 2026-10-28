//! The runtime half of [`crate::future`]: futures Rawr code holds, and
//! [`Script`], which polls one as a std future.

// Without `sync` the values are not `Send`, and one thread runs everything.
#![cfg_attr(not(feature = "sync"), allow(clippy::arc_with_non_send_sync))]

use std::cell::{Cell, RefCell};
#[cfg(feature = "sync")]
use std::collections::VecDeque;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::task::{Context, Waker};

use super::{Frame, Poll};

use crate::{item, Dynamic, EvalAltResult, Position, RawrResultOf};

/// What a native future gives when it ends: a function that makes its Rawr
/// value on the owner thread, where imported values are stored.
#[cfg(feature = "sync")]
pub type Finish = Box<dyn FnOnce() -> RawrResultOf<Dynamic> + Send>;
/// What a native future gives when it ends.
#[cfg(not(feature = "sync"))]
pub type Finish = Box<dyn FnOnce() -> RawrResultOf<Dynamic>>;

/// A native future, made on the thread that first polls it.
#[cfg(feature = "sync")]
pub type Native = Pin<Box<dyn std::future::Future<Output = Finish> + Send>>;
/// A native future, made on the thread that first polls it.
#[cfg(not(feature = "sync"))]
pub type Native = Pin<Box<dyn std::future::Future<Output = Finish>>>;

/// What makes a native future.
#[cfg(feature = "sync")]
type Make = Box<dyn FnOnce() -> Native + Send>;
#[cfg(not(feature = "sync"))]
type Make = Box<dyn FnOnce() -> Native>;

/// Work for the owner thread, boxed.
#[cfg(feature = "sync")]
type Work = Box<dyn FnOnce() + Send>;
#[cfg(not(feature = "sync"))]
type Work = Box<dyn FnOnce()>;

/// `Send` when the crate is built with `sync`, as the values are.
#[cfg(feature = "sync")]
pub trait MaybeSend: Send {}
#[cfg(feature = "sync")]
impl<T: Send> MaybeSend for T {}
/// `Send` when the crate is built with `sync`, as the values are.
#[cfg(not(feature = "sync"))]
pub trait MaybeSend {}
#[cfg(not(feature = "sync"))]
impl<T> MaybeSend for T {}

/// A future as Rawr code holds it.
#[derive(Clone)]
pub struct Future(Arc<Mutex<Option<Kind>>>);

enum Kind {
    /// An async body's step function, not yet started.
    Script(item::Value),
    /// A native future, made when first polled.
    Native(Make),
    /// Futures polled together; their outputs in order, as a tuple.
    Join(Vec<Future>),
    /// A value already there.
    Ready(Dynamic),
}

impl std::fmt::Debug for Future {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Future")
    }
}

impl Future {
    /// The future of an async body whose step function is `step`: called
    /// with `()` it starts, and called with a [`Frame`] it goes on.
    #[must_use]
    pub fn script(step: item::Value) -> Self {
        Self::of(Kind::Script(step))
    }

    /// A native future that `make` makes on the thread that first polls it.
    #[must_use]
    pub fn native(make: impl FnOnce() -> Native + MaybeSend + 'static) -> Self {
        Self::of(Kind::Native(Box::new(make)))
    }

    /// The futures in `parts`, polled together; their outputs make one
    /// tuple, in order, as `join!` gives them.
    #[must_use]
    pub fn join(parts: Vec<Self>) -> Self {
        Self::of(Kind::Join(parts))
    }

    /// A future whose output is `value`.
    #[must_use]
    pub fn ready(value: Dynamic) -> Self {
        Self::of(Kind::Ready(value))
    }

    fn of(kind: Kind) -> Self {
        Self(Arc::new(Mutex::new(Some(kind))))
    }

    /// Run this future as a std future. Its first run takes it: a copy
    /// that runs again panics, as a moved future can't be polled twice.
    #[must_use]
    pub fn into_script(self) -> Script {
        Script {
            poller: Poller::from(&self),
        }
    }
}

/// A Rawr future as a [`std::future::Future`]. It is `Send` when the crate
/// is built with `sync`.
pub struct Script {
    poller: Poller,
}

impl std::fmt::Debug for Script {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Script")
    }
}

impl Script {
    /// Poll for the Rawr output, or the error that ended it.
    pub fn poll_output(&mut self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        self.poller.poll(cx)
    }
}

impl std::future::Future for Script {
    type Output = crate::Value;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {
        match self.poller.poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Pending,
            std::task::Poll::Ready(Ok(value)) => std::task::Poll::Ready(crate::Value::from(value)),
            std::task::Poll::Ready(Err(error)) => panic!("{}", message(&error)),
        }
    }
}

/// The text of a script failure, as `rawr run` prints it.
#[must_use]
pub fn message(error: &EvalAltResult) -> String {
    match error {
        EvalAltResult::ErrorRuntime(value, ..) if value.is_string() => value.to_string(),
        error => error.to_string(),
    }
}

/// A run-time failure with `message`, as a native future's output gives
/// one.
pub fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    failure(message)
}

fn failure(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

enum Poller {
    Script(Box<Stepper>),
    Native {
        make: Option<Make>,
        running: Option<Native>,
        made: Option<Arc<Slot>>,
    },
    /// Each part until it ends, then its output: a part that ended is
    /// dropped then, with what it captured, as `join!` drops it.
    Join(Vec<(Option<Poller>, Option<Dynamic>)>),
    Ready(Option<Dynamic>),
    Failed(Option<Box<EvalAltResult>>),
}

impl From<&Future> for Poller {
    fn from(future: &Future) -> Self {
        let kind = future
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        match kind {
            None => Self::Failed(Some(failure("rawr: this future was already awaited"))),
            Some(Kind::Script(step)) => Self::Script(Box::new(Stepper::new(step))),
            Some(Kind::Native(make)) => Self::Native {
                make: Some(make),
                running: None,
                made: None,
            },
            Some(Kind::Join(parts)) => Self::Join(
                parts
                    .iter()
                    .map(|part| (Some(Self::from(part)), None))
                    .collect(),
            ),
            Some(Kind::Ready(value)) => Self::Ready(Some(value)),
        }
    }
}

impl Poller {
    fn poll(&mut self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        use std::task::Poll::{Pending, Ready};
        match self {
            Self::Script(stepper) => stepper.poll(cx),
            Self::Native {
                make,
                running,
                made,
            } => {
                if let Some(slot) = made {
                    return slot.poll(cx);
                }
                if running.is_none() {
                    let Some(make) = make.take() else {
                        return Ready(Err(failure(
                            "rawr: a native future was polled after it ended",
                        )));
                    };
                    *running = Some(make());
                }
                let Some(future) = running.as_mut() else {
                    return Pending;
                };
                match future.as_mut().poll(cx) {
                    Pending => Pending,
                    Ready(finish) => {
                        *running = None;
                        let slot = Slot::run(finish, cx.waker());
                        let output = slot.poll(cx);
                        *made = Some(slot);
                        output
                    }
                }
            }
            Self::Join(parts) => {
                let mut pending = false;
                for (part, output) in parts.iter_mut() {
                    let Some(poller) = part.as_mut() else {
                        continue;
                    };
                    match poller.poll(cx) {
                        Pending => pending = true,
                        Ready(Ok(value)) => {
                            *output = Some(value);
                            *part = None;
                        }
                        Ready(Err(error)) => return Ready(Err(error)),
                    }
                }
                if pending {
                    return Pending;
                }
                Ready(Ok(Dynamic::from_array(
                    parts
                        .iter_mut()
                        .filter_map(|(_, output)| output.take())
                        .collect(),
                )))
            }
            Self::Ready(value) => match value.take() {
                Some(value) => Ready(Ok(value)),
                None => Ready(Err(failure("rawr: a future was polled after it ended"))),
            },
            Self::Failed(error) => Ready(Err(error
                .take()
                .unwrap_or_else(|| failure("rawr: a future was polled after it ended")))),
        }
    }
}

/// An async body run step by step on the owner thread.
struct Stepper {
    shared: Arc<Mutex<Step>>,
    awaiting: Option<Box<Poller>>,
}

struct Step {
    /// The step function, with what the body captured; dropped when the
    /// body ends, as Rust drops a finished future's state.
    step: Option<item::Value>,
    /// The suspended body, once it has awaited.
    frame: Option<Frame>,
    /// The awaited future's output, to resume with.
    input: Option<Dynamic>,
    state: State,
    waker: Option<Waker>,
}

enum State {
    /// Ready to run its next step.
    Idle,
    /// Its next step waits for the owner.
    Queued,
    /// The step awaits this future.
    Awaiting(Future),
    /// The body ended.
    Done(RawrResultOf<Dynamic>),
    /// A step panicked; its poller panics with the same payload.
    Panicked(Box<dyn std::any::Any + Send>),
    /// The output was taken.
    Taken,
}

impl Stepper {
    fn new(step: item::Value) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Step {
                step: Some(step),
                frame: None,
                input: None,
                state: State::Idle,
                waker: None,
            })),
            awaiting: None,
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        use std::task::Poll::{Pending, Ready};
        loop {
            if let Some(awaiting) = self.awaiting.as_mut() {
                let output = match awaiting.poll(cx) {
                    Pending => return Pending,
                    Ready(output) => output,
                };
                self.awaiting = None;
                let mut step = lock(&self.shared);
                match output {
                    Ok(value) => {
                        step.input = Some(value);
                        step.state = State::Idle;
                    }
                    Err(error) => {
                        step.state = State::Taken;
                        return Ready(Err(error));
                    }
                }
            }
            let mut step = lock(&self.shared);
            match std::mem::replace(&mut step.state, State::Taken) {
                State::Idle => {
                    if owns() {
                        drop(step);
                        run(&self.shared);
                    } else {
                        step.state = State::Queued;
                        step.waker = Some(cx.waker().clone());
                        drop(step);
                        // A queued step holds its body weakly: a future
                        // dropped before the owner reaches it runs no more.
                        let shared = Arc::downgrade(&self.shared);
                        send(Job::new(move || {
                            let Some(shared) = shared.upgrade() else {
                                return;
                            };
                            run(&shared);
                            let waker = lock(&shared).waker.take();
                            drop(shared);
                            if let Some(waker) = waker {
                                waker.wake();
                            }
                        }));
                        return Pending;
                    }
                }
                State::Queued => {
                    step.state = State::Queued;
                    step.waker = Some(cx.waker().clone());
                    return Pending;
                }
                State::Awaiting(future) => {
                    drop(step);
                    self.awaiting = Some(Box::new(Poller::from(&future)));
                }
                State::Done(output) => return Ready(output),
                State::Panicked(payload) => std::panic::resume_unwind(payload),
                State::Taken => {
                    return Ready(Err(failure("rawr: a future was polled after it ended")))
                }
            }
        }
    }
}

impl Drop for Stepper {
    /// A body dropped before it ends, as a cancelled task's is, drops what it
    /// holds on the owner thread: a suspended `for` loop's iterator can only
    /// be dropped where it was made.
    fn drop(&mut self) {
        if owns() {
            return;
        }
        let mut step = lock(&self.shared);
        let held = (step.frame.take(), step.step.take(), step.input.take());
        drop(step);
        if held.0.is_some() || held.1.is_some() || held.2.is_some() {
            send(Job::new(move || drop(held)));
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Run one step of the body in `shared` on this, the owner thread.
fn run(shared: &Mutex<Step>) {
    let (step, argument) = {
        let mut state = lock(shared);
        let argument = match (state.frame.take(), state.input.take()) {
            (Some(frame), input) => frame.resume(input.unwrap_or(Dynamic::UNIT)),
            (None, _) => Dynamic::UNIT,
        };
        let Some(step) = state.step.clone() else {
            state.state = State::Done(Err(failure("rawr: a future was polled after it ended")));
            return;
        };
        (step, argument)
    };
    // A panic in a step, as native code's, is its future's: it reaches the
    // thread that polls it, not the owner's runtime.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_host(|host| host.invoke(&step, &mut [argument]))
            .unwrap_or_else(|| Err(failure("rawr: no program runs async code on this thread")))
    }));
    let mut state = lock(shared);
    let state = &mut *state;
    let result = match result {
        Ok(result) => result,
        Err(payload) => {
            state.state = State::Panicked(payload);
            state.step = None;
            state.frame = None;
            return;
        }
    };
    state.state = match result {
        Err(error) => State::Done(Err(error)),
        // A body that never awaits gives its output as it is.
        Ok(output) if !output.is::<Poll>() => State::Done(Ok(output)),
        Ok(output) => match output.cast::<Poll>() {
            Poll::Ready(value) => State::Done(Ok(value)),
            Poll::Pending { future, frame } => {
                state.frame = Some(frame);
                match future.try_cast::<Future>() {
                    Some(future) => State::Awaiting(future),
                    None => State::Done(Err(failure(
                        "rawr: an async body awaited a value that is not a future",
                    ))),
                }
            }
        },
    };
    if matches!(state.state, State::Done(_)) {
        state.step = None;
        state.frame = None;
    }
}

/// A native output made a Rawr value on the owner thread.
struct Slot {
    output: Mutex<(Option<RawrResultOf<Dynamic>>, Option<Waker>)>,
}

impl Slot {
    fn run(finish: Finish, waker: &Waker) -> Arc<Self> {
        let slot = Arc::new(Self {
            output: Mutex::new((None, Some(waker.clone()))),
        });
        if owns() {
            lock(&slot.output).0 = Some(finish());
        } else {
            let shared = Arc::downgrade(&slot);
            send(Job::new(move || {
                let Some(shared) = shared.upgrade() else {
                    return;
                };
                let output = finish();
                let waker = {
                    let mut state = lock(&shared.output);
                    state.0 = Some(output);
                    state.1.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            }));
        }
        slot
    }

    fn poll(&self, cx: &mut Context<'_>) -> std::task::Poll<RawrResultOf<Dynamic>> {
        let mut state = lock(&self.output);
        match state.0.take() {
            Some(output) => std::task::Poll::Ready(output),
            None => {
                state.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }
}

/// The program an owner thread runs async steps in.
pub trait Host {
    /// Call `function` with `arguments` in the current generation.
    fn invoke(&self, function: &item::Value, arguments: &mut [Dynamic]) -> RawrResultOf<Dynamic>;

    /// The program as any thread may call it, each call with an evaluation
    /// of its own (D490), for a script closure native code calls.
    fn share(&self) -> Option<Program> {
        None
    }
}

thread_local! {
    static HOST: RefCell<Option<Rc<dyn Host>>> = const { RefCell::new(None) };
}

/// The running program as any thread may use it: each call runs with an
/// evaluation of its own (D490).
pub trait Remote: crate::func::SendSync {
    /// Call `function` with `arguments`.
    fn invoke(&self, function: &item::Value, arguments: &mut [Dynamic]) -> RawrResultOf<Dynamic>;

    /// Check `value` against the exact type `witness` proves.
    fn validate(
        &self,
        witness: &crate::host::runtime::Witness,
        value: &Dynamic,
    ) -> Result<(), String>;

    /// A copy of `value` made by its type's own `Clone`, when the program
    /// wrote one; `None` for a derived `Clone`, which copies the value.
    fn clone_value(&self, value: &Dynamic) -> Option<RawrResultOf<Dynamic>> {
        let _ = value;
        None
    }

    /// `value`'s `Debug` text as its derived `Debug` writes it.
    fn debug(&self, value: &Dynamic) -> Option<String> {
        let _ = value;
        None
    }

    /// `lhs == rhs` by their script type's own `PartialEq`, when the
    /// program wrote one, and else as a derived one compares them.
    fn equal(&self, lhs: &Dynamic, rhs: &Dynamic) -> Option<RawrResultOf<bool>> {
        let _ = (lhs, rhs);
        None
    }

    /// `value`'s text as its script type's own `Display` writes it; `None`
    /// when the type has none.
    fn display(&self, value: &Dynamic) -> Option<RawrResultOf<String>> {
        let _ = value;
        None
    }

    /// The `Default` of the script type at `path` (`State`, `app::State`):
    /// its own `impl Default`, or the derived one (D594). `None` when the
    /// program has no such type.
    fn default_of(&self, path: &str) -> Option<RawrResultOf<Dynamic>> {
        let _ = path;
        None
    }

    /// Whether `function`, a callable `from` made, runs here: the same
    /// item by its stable key and signature, and for a lifted closure the
    /// same closures in the function that writes it (D553.5).
    fn rebinds(&self, function: &item::Value, from: &dyn Remote) -> bool {
        let _ = (function, from);
        false
    }

    /// The closures written by the function that writes `closure`, each by
    /// its key and signature, which carries its captures' names and types.
    /// `None` when this program can't say.
    fn closures(&self, closure: &item::Key) -> Option<&[item::Value]> {
        let _ = closure;
        None
    }
}

/// A program any thread may call into.
pub type Program = Arc<dyn Remote>;

thread_local! {
    /// How many calls from native code into script code this thread is
    /// inside (D553.1). A marked `main`'s own frame is not one.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// The latest generation offered to the thread that owns the program, as
/// what admits it there. A newer offer replaces an older one.
#[allow(clippy::type_complexity)]
static OFFERED: Mutex<Option<Box<dyn FnOnce() + Send>>> = Mutex::new(None);
/// Whether [`OFFERED`] holds an offer, read at each entry.
static WAITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether native code has called into script code in this process.
static ENTERED_ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether native code has called into script code in this process, so a
/// generation [`offer`]ed can be admitted.
#[must_use]
pub fn called_from_native() -> bool {
    ENTERED_ONCE.load(std::sync::atomic::Ordering::Acquire)
}

/// Offer a committed generation to the thread that owns the program
/// (D553.3): `admit` runs there at its next outermost entry from native
/// code, so the whole call tree of each entry runs in one generation. The
/// latest offer wins.
pub fn offer(admit: impl FnOnce() + Send + 'static) {
    let older = {
        let mut offered = lock(&OFFERED);
        WAITING.store(true, std::sync::atomic::Ordering::Release);
        offered.replace(Box::new(admit))
    };
    drop(older);
}

/// Inside a call from native code into script code until it drops
/// (D553.1). The outermost one on the thread that owns the program first
/// admits the generation [`offer`]ed last.
#[must_use]
pub fn entry() -> Entry {
    let depth = DEPTH.with(|depth| depth.replace(depth.get() + 1));
    if depth == 0 && !ENTERED_ONCE.load(std::sync::atomic::Ordering::Relaxed) {
        ENTERED_ONCE.store(true, std::sync::atomic::Ordering::Release);
    }
    if depth == 0 && WAITING.load(std::sync::atomic::Ordering::Acquire) && owns() {
        let admit = {
            let mut offered = lock(&OFFERED);
            WAITING.store(false, std::sync::atomic::Ordering::Release);
            offered.take()
        };
        // In a memoizing framework, such as Freya, nothing calls the new
        // generation until something marks a scope dirty: an edit shows at
        // the next event or state change, not at once. Showing it at once
        // needs a framework API that marks every scope dirty and keeps its
        // hooks, which Freya doesn't have (`Runner::reload` resets hooks).
        // D553.7 logs this for a later revisit, such as an upstream
        // proposal.
        if let Some(admit) = admit {
            admit();
        }
    }
    Entry(())
}

/// The guard [`entry`] gives.
pub struct Entry(());

impl Drop for Entry {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Whether this thread is outside every call from native code into script
/// code: a marked `main`'s own frame runs here.
#[must_use]
pub fn outermost() -> bool {
    DEPTH.with(Cell::get) == 0
}

/// A callable a marked `main`'s own frame gave native code. `main` never
/// runs again to make another, so a generation it can't run in restarts
/// the program (D553.5).
pub struct Held {
    function: item::Value,
    program: Program,
}

/// Each callable a marked `main`'s own frame gave native code, while
/// native code holds it.
#[cfg(feature = "sync")]
static HELD: Mutex<Vec<std::sync::Weak<Held>>> = Mutex::new(Vec::new());

#[cfg(not(feature = "sync"))]
thread_local! {
    static HELD: RefCell<Vec<std::sync::Weak<Held>>> = const { RefCell::new(Vec::new()) };
}

fn with_held<R>(work: impl FnOnce(&mut Vec<std::sync::Weak<Held>>) -> R) -> R {
    #[cfg(feature = "sync")]
    return work(&mut lock(&HELD));
    #[cfg(not(feature = "sync"))]
    return HELD.with_borrow_mut(work);
}

/// Each callable a marked `main`'s own frame gave native code that native
/// code still holds, with the program that made it.
pub fn held() -> Vec<(item::Value, Program)> {
    with_held(|held| {
        held.retain(|held| held.strong_count() > 0);
        held.iter()
            .filter_map(std::sync::Weak::upgrade)
            .map(|held| (held.function.clone(), held.program.clone()))
            .collect()
    })
}

/// The program the process runs, for script values native code holds
/// (`rawr::Value`'s `Clone` and `Debug`) on any thread.
#[cfg(feature = "sync")]
static PUBLISHED: std::sync::RwLock<Option<Program>> = std::sync::RwLock::new(None);

/// Make `program` the one script values native code holds belong to.
pub fn publish(program: Program) {
    #[cfg(feature = "sync")]
    {
        *PUBLISHED.write().unwrap_or_else(PoisonError::into_inner) = Some(program);
    }
    #[cfg(not(feature = "sync"))]
    let _ = program;
}

fn published() -> Option<Program> {
    #[cfg(feature = "sync")]
    return PUBLISHED.read().ok()?.clone();
    #[cfg(not(feature = "sync"))]
    return None;
}

/// The program script code runs in on this thread: the generation current
/// here, and else the one the process published.
#[cfg(feature = "sync")]
pub(crate) fn current_program() -> Option<Program> {
    with_host(|host| host.share()).flatten().or_else(published)
}

/// A copy of `value` by its script type's own `Clone` (review 8).
pub(crate) fn clone_value(value: &Dynamic) -> Option<RawrResultOf<Dynamic>> {
    published()?.clone_value(value)
}

/// A script closure native code calls (D513.7 b): `F: FnOnce() -> R` takes
/// one, as `spawn_blocking` does. Each call runs the closure with what it
/// captured, in an evaluation of its own on the calling thread (D490). The
/// imported values a closure captured live on the owner thread, so a call
/// on another thread can't reach them.
///
/// A call from native code is an entry (D553.1): the outermost one admits
/// a newer generation, and the closure runs there when it rebinds by its
/// stable identity. Otherwise it runs in the generation that made it.
#[derive(Clone)]
pub struct Callable {
    function: item::Value,
    program: Program,
    /// Set when a marked `main`'s own frame made it.
    _held: Option<Arc<Held>>,
}

impl std::fmt::Debug for Callable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Callable")
    }
}

impl Callable {
    /// The script closure `function` holds, on the owner thread.
    pub fn new(function: Dynamic) -> RawrResultOf<Self> {
        let actual = function.type_name();
        let function = function
            .try_cast_result::<item::Value>()
            .map_err(|_| failure(format!("expected a closure, found `{actual}`")))?;
        let program = with_host(|host| host.share())
            .flatten()
            .ok_or_else(|| failure("rawr: no program runs closures on this thread"))?;
        let held = outermost().then(|| {
            let held = Arc::new(Held {
                function: function.bind(Vec::new()),
                program: program.clone(),
            });
            with_held(|all| all.push(Arc::downgrade(&held)));
            held
        });
        Ok(Self {
            function,
            program,
            _held: held,
        })
    }

    /// The program this call runs in: the generation current on this
    /// thread when the closure rebinds there, and else the one that made it.
    fn program(&self) -> Program {
        match with_host(|host| host.share()).flatten() {
            Some(current)
                if !std::ptr::addr_eq(Arc::as_ptr(&current), Arc::as_ptr(&self.program))
                    && current.rebinds(&self.function, self.program.as_ref()) =>
            {
                current
            }
            _ => self.program.clone(),
        }
    }

    /// Call the closure with `arguments`.
    pub fn call(&self, arguments: &mut [Dynamic]) -> RawrResultOf<Dynamic> {
        let _entry = entry();
        self.program().invoke(&self.function, arguments)
    }

    /// Call the closure with `arguments`, where each index in `mutable` is
    /// an argument it takes `&mut` (D594). A function changes its first
    /// `&mut` parameter in place and gives the others back after its value,
    /// as `[value, ..copied]`; a method or an associated function gives back
    /// all but its receiver, and a closure all of them
    /// (`resolve::lower_nominal::copied_parameters`). Each given back is
    /// stored in its argument's place, so the caller finds every `&mut`
    /// argument where it lent it.
    pub fn call_places(
        &self,
        arguments: &mut [Dynamic],
        mutable: &[usize],
    ) -> RawrResultOf<Dynamic> {
        let _entry = entry();
        let program = self.program();
        let result = program.invoke(&self.function, arguments)?;
        let copied = match self.function.kind() {
            item::Kind::Function
                if self.function.captures().is_empty()
                    && !self.function.key().name().contains("{closure#") =>
            {
                mutable.get(1..).unwrap_or_default()
            }
            item::Kind::Method => mutable
                .iter()
                .position(|index| *index != 0)
                .map_or(&[][..], |at| &mutable[at..]),
            _ => mutable,
        };
        if copied.is_empty() {
            return Ok(result);
        }
        let actual = result.type_name();
        let returned = result
            .into_array()
            .ok()
            .filter(|returned| returned.len() == copied.len() + 1)
            .ok_or_else(|| {
                failure(format!(
                    "rawr: a function native code called gave back `{actual}`, not its `&mut` arguments"
                ))
            })?;
        let mut returned = returned.into_iter();
        let result = returned.next().unwrap_or(Dynamic::UNIT);
        for (index, value) in copied.iter().zip(returned) {
            if let Some(place) = arguments.get_mut(*index) {
                *place = value;
            }
        }
        Ok(result)
    }

    /// Call the closure with `arguments`, each first checked against the
    /// witness of the closure's parameter at its place: native code that
    /// gives a closure a value of another type is refused (review 2).
    pub fn call_checked(
        &self,
        arguments: &mut [Dynamic],
        witnesses: &[Option<crate::host::runtime::Witness>],
    ) -> RawrResultOf<Dynamic> {
        let _entry = entry();
        let program = self.program();
        for (index, (argument, witness)) in arguments.iter().zip(witnesses).enumerate() {
            if let Some(witness) = witness {
                program.validate(witness, argument).map_err(|reason| {
                    failure(format!(
                        "rawr: native code gave a closure's argument {index} a value that is not its type: {reason}"
                    ))
                })?;
            }
        }
        program.invoke(&self.function, arguments)
    }
}

/// Work for the owner thread.
pub struct Job(Work);

impl Job {
    fn new(work: impl FnOnce() + MaybeSend + 'static) -> Self {
        Self(Box::new(work))
    }

    /// Run it, on the owner thread.
    pub fn run(self) {
        (self.0)();
    }
}

#[allow(clippy::type_complexity)]
static FORWARD: OnceLock<Box<dyn Fn(Job) + Send + Sync>> = OnceLock::new();

/// Jobs for an owner that drives them itself, as the thread running the
/// runtime entry's `block_on` does (D527): it drains them in its own poll.
#[cfg(feature = "sync")]
static QUEUE: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
/// What wakes that owner when a job arrives.
#[cfg(feature = "sync")]
static DRIVER: Mutex<Option<Waker>> = Mutex::new(None);

thread_local! {
    /// Whether this thread is inside the runtime entry's poll (D527).
    static ENTERED: Cell<bool> = const { Cell::new(false) };
}

/// Make the owner the thread that polls with `waker`, as the runtime entry
/// does each time it polls the program's `main`: jobs from other threads
/// queue for [`drain`] and wake it (D527).
#[cfg(feature = "sync")]
pub fn drive(waker: &Waker) {
    *lock(&DRIVER) = Some(waker.clone());
}

/// Run every job queued for the owner, in order, on this, the owner thread.
pub fn drain() {
    #[cfg(feature = "sync")]
    loop {
        let job = lock(&QUEUE).pop_front();
        match job {
            Some(job) => job.run(),
            None => break,
        }
    }
}

/// This thread is inside the runtime entry's poll until the guard drops: a
/// native function that returns a future runs at once, in the runtime's
/// context, as Rust calls it (D527).
#[must_use]
pub fn enter() -> Entered {
    Entered(ENTERED.with(|entered| entered.replace(true)))
}

/// The guard [`enter`] gives.
pub struct Entered(bool);

impl Drop for Entered {
    fn drop(&mut self) {
        ENTERED.with(|entered| entered.set(self.0));
    }
}

/// Whether this thread runs inside the runtime entry's poll (D527).
#[must_use]
pub fn entered() -> bool {
    ENTERED.with(Cell::get)
}

/// Make this thread the owner that runs async steps in `host`, from now on.
pub fn serve(host: Rc<dyn Host>) {
    HOST.with(|slot| *slot.borrow_mut() = Some(host));
}

/// Where a step polled on another thread goes: `forward` hands it to the
/// owner thread, which [`Job::run`]s it. Set once.
pub fn forward(forward: impl Fn(Job) + Send + Sync + 'static) {
    let _ = FORWARD.set(Box::new(forward));
}

/// Run `work` on the owner thread: here when this is it, and else as a
/// [`Job`] it runs next. A native value that leaves its slot for a future
/// goes back this way when the future ends on another thread (D513.6).
pub fn on_owner(work: impl FnOnce() + MaybeSend + 'static) {
    if owns() {
        work();
    } else {
        send(Job::new(work));
    }
}

/// Whether this thread runs async steps.
#[must_use]
pub fn owns() -> bool {
    HOST.with(|slot| slot.borrow().is_some())
}

fn with_host<T>(work: impl FnOnce(&dyn Host) -> T) -> Option<T> {
    let host = HOST.with(|slot| slot.borrow().clone())?;
    Some(work(host.as_ref()))
}

/// `value`'s `Debug` text through the program it belongs to (review 8).
pub(crate) fn debug(value: &Dynamic) -> Option<String> {
    published()?.debug(value)
}

/// `lhs == rhs` by their script type's `PartialEq`, through the program
/// they belong to.
#[cfg(feature = "sync")]
pub(crate) fn equal(lhs: &Dynamic, rhs: &Dynamic) -> Option<RawrResultOf<bool>> {
    published()?.equal(lhs, rhs)
}

/// `value`'s text by its script type's own `Display`, through the program
/// it belongs to.
#[cfg(feature = "sync")]
pub(crate) fn display(value: &Dynamic) -> Option<RawrResultOf<String>> {
    published()?.display(value)
}

fn send(job: Job) {
    #[cfg(feature = "sync")]
    {
        let driver = lock(&DRIVER).clone();
        if let Some(driver) = driver {
            lock(&QUEUE).push_back(job);
            driver.wake();
            return;
        }
    }
    match FORWARD.get() {
        Some(forward) => forward(job),
        // No owner serves this process: the job runs where it is, and
        // fails there when it needs the program.
        None => job.run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_native_future_runs_where_it_is_polled_and_its_output_is_made_on_the_owner() {
        struct Plain;
        impl Host for Plain {
            fn invoke(&self, _: &item::Value, _: &mut [Dynamic]) -> RawrResultOf<Dynamic> {
                unreachable!()
            }
        }
        serve(Rc::new(Plain));
        let future = Future::join(vec![
            Future::native(|| Box::pin(async { Box::new(|| Ok(Dynamic::from(1_i64))) as Finish })),
            Future::ready(Dynamic::from(2_i64)),
        ]);
        let mut script = future.clone().into_script();
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let std::task::Poll::Ready(Ok(output)) = script.poll_output(&mut cx) else {
            panic!("the join is ready");
        };
        assert_eq!(format!("{output:?}"), "[1, 2]");
        // A copy shares the future, which runs once.
        let mut again = future.into_script();
        assert!(matches!(
            again.poll_output(&mut cx),
            std::task::Poll::Ready(Err(_))
        ));
    }
}
