//! Durable cloneable application task plans.

use rawr::{application, host, item, preview, program, Dynamic, EvalAltResult, Position};
use std::fmt;

use crate::runtime;

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
}

/// iced's own tasks: `exit`, `window::latest`/`close` and the focus
/// operations. They run in iced's runtime and touch no Rawr authority.
#[derive(Clone, Debug)]
pub(crate) enum Native {
    Exit,
    LatestWindow,
    CloseWindow(iced::window::Id),
    FocusNext,
    FocusPrevious,
    Focus(iced::widget::Id),
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
            Self::Exit => iced::exit(),
            Self::LatestWindow => {
                iced::window::latest().map(|id| Ok(rawr::option::from(id)))
            }
            Self::CloseWindow(id) => iced::window::close(id),
            Self::FocusNext => iced::widget::operation::focus_next(),
            Self::FocusPrevious => iced::widget::operation::focus_previous(),
            Self::Focus(id) => iced::widget::operation::focus(id),
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
            | Kind::AndThen { .. } => None,
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
        let finish = authority.clone();
        Ok(self.lower_payload(authority)?.map(move |result| {
            match result.and_then(|payload| finish.finish(payload)) {
                Ok(payload) => runtime::Message::new(instance, generation, payload),
                Err(error) => runtime::Message::task_error(instance, generation, error),
            }
        }))
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
        };
        Ok(Prepared { kind, witness })
    }

    fn lower_payload(
        self,
        authority: &Authority,
    ) -> Result<iced::Task<Result<Dynamic, Error>>, Error> {
        let Self { kind, witness } = self;
        let task = match kind {
            Kind::None => iced::Task::none(),
            Kind::Done(payload) => iced::Task::done(Ok(payload)),
            Kind::Perform { call, mapper } => {
                let pending = authority.issue(*call, &mapper).map_err(Error::Operation)?;
                iced::Task::perform(pending, |result| result.map_err(Error::Operation))
            }
            Kind::Batch(plans) => plans
                .into_vec()
                .into_iter()
                .map(|plan| plan.lower_payload(authority))
                .collect::<Result<Vec<_>, _>>()
                .map(iced::Task::batch)?,
            Kind::Chain { first, second } => {
                let first = first.lower_payload(authority)?;
                let second = second.lower_payload(authority)?;
                first.chain(second)
            }
            Kind::Map { plan, mapper } => {
                let authority = authority.clone();
                plan.lower_payload(&authority)?
                    .map(move |result| result.and_then(|payload| authority.map(payload, &mapper)))
            }
            Kind::Native(native) => native.lower(),
            Kind::AndThen { plan, mapper } => {
                and_then(plan.lower_payload(authority)?, authority.clone(), mapper)
            }
        };
        let Some(witness) = witness else {
            return Ok(task);
        };
        let authority = authority.clone();
        Ok(task.map(move |result| {
            result.and_then(|payload| authority.validate_output(&witness, payload))
        }))
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
        let finish = authority.clone();
        self.lower_payload(context, &authority).map(move |result| {
            match result.and_then(|payload| finish.finish(payload)) {
                Ok(payload) => runtime::Message::new(instance, generation, payload),
                Err(error) => runtime::Message::task_error(instance, generation, error),
            }
        })
    }

    fn lower_payload(
        self,
        context: &rawr::reload::installation::installed::Context<'_>,
        authority: &Authority,
    ) -> iced::Task<Result<Dynamic, Error>> {
        let Self { kind, witness } = self;
        let task = match kind {
            PreparedKind::None => iced::Task::none(),
            PreparedKind::Done(payload) => iced::Task::done(Ok(payload)),
            PreparedKind::Perform { pending } => {
                let pending = context.operation(pending);
                iced::Task::perform(pending, |result| result.map_err(Error::Operation))
            }
            PreparedKind::Batch(plans) => iced::Task::batch(
                plans
                    .into_vec()
                    .into_iter()
                    .map(|plan| plan.lower_payload(context, authority)),
            ),
            PreparedKind::Chain { first, second } => first
                .lower_payload(context, authority)
                .chain(second.lower_payload(context, authority)),
            PreparedKind::Map { plan, mapper } => {
                let authority = authority.clone();
                plan.lower_payload(context, &authority)
                    .map(move |result| result.and_then(|payload| authority.map(payload, &mapper)))
            }
            PreparedKind::Native(native) => native.lower(),
            PreparedKind::AndThen { plan, mapper } => {
                and_then(plan.lower_payload(context, authority), authority.clone(), mapper)
            }
        };
        let Some(witness) = witness else {
            return task;
        };
        let authority = authority.clone();
        task.map(move |result| {
            result.and_then(|payload| authority.validate_output(&witness, payload))
        })
    }
}

/// Each `Some` the task yields, given to `mapper`, whose plan then runs.
fn and_then(
    task: iced::Task<Result<Dynamic, Error>>,
    authority: Authority,
    mapper: item::Value,
) -> iced::Task<Result<Dynamic, Error>> {
    task.then(move |result| {
        let mut payload = match result {
            Ok(payload) => payload,
            Err(error) => return iced::Task::done(Err(error)),
        };
        let actual = payload.type_name().to_string();
        let inner = match rawr::option::take(&mut payload) {
            Some(Some(inner)) => inner,
            Some(None) => return iced::Task::none(),
            None => return iced::Task::done(Err(Error::Representation { actual })),
        };
        let next = authority
            .map(inner, &mapper)
            .and_then(Plan::from_dynamic)
            .and_then(|plan| plan.lower_payload(&authority));
        match next {
            Ok(task) => task,
            Err(error) => iced::Task::done(Err(error)),
        }
    })
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
            native_task(call, Native::Focus(take_value(id, "widget::Id")?))
        })
        .expect("the iced facade must retain operation::focus");
    for name in ["snap_to", "snap_to_end", "scroll_to", "scroll_by"] {
        let path = format!("iced::widget::operation::scrollable::{name}");
        builder
            .bind_with_context(path.clone(), move |call, args| {
                let expected = if name == "snap_to_end" { 2 } else { 3 };
                if args.len() != expected {
                    return Err(arity(&path, expected, args.len()));
                }
                let id = take_value::<iced::widget::Id>(args[0], "widget::Id")?;
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
        iced_test::futures::futures::executor::block_on(async move {
            match stream.next().await {
                Some(iced_test::runtime::Action::Output(message)) => message,
                Some(action) => panic!("expected message output, found {action:?}"),
                None => panic!("operation task must emit one message"),
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
