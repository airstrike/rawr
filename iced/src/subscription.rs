//! Durable cloneable application subscription plans and iced recipes.

use rawr::{application, host, item, program, Dynamic, EvalAltResult, Position};
use std::fmt;
use std::hash::Hash;

use crate::runtime;

/// A durable subscription plan could not cross or bind at the native boundary.
#[derive(Debug)]
pub enum Error {
    /// A runtime value was not the exact durable subscription-plan carrier.
    Representation {
        /// Actual runtime type name.
        actual: String,
    },
    /// Exact retained authority could not bind one symbolic stream recipe.
    Stream(host::stream::Error),
}

pub(crate) fn from_dynamic(value: Dynamic) -> Result<host::stream::Plan, Error> {
    let actual = value.type_name().to_string();
    value
        .try_cast_result::<host::stream::Plan>()
        .map_err(|_| Error::Representation { actual })
}

pub(crate) fn lower<Message, F>(
    recipes: Result<Box<[host::stream::Recipe]>, host::stream::Error>,
    instance: application::Instance,
    generation: program::GenerationId,
    emit: F,
) -> iced::Subscription<Message>
where
    Message: Send + 'static,
    F: Fn(runtime::Message) -> Message + Clone + Send + 'static,
{
    match recipes {
        Ok(recipes) => iced::Subscription::batch(recipes.into_vec().into_iter().map(|recipe| {
            iced::advanced::subscription::from_recipe(Bound {
                instance,
                generation,
                recipe,
                emit: emit.clone(),
                output: std::marker::PhantomData,
            })
        })),
        Err(error) => iced::advanced::subscription::from_recipe(Failed {
            instance,
            generation,
            error: Error::Stream(error),
            emit,
            output: std::marker::PhantomData,
        }),
    }
}

thread_local! {
    /// iced's event input for the recipe starting on this thread.
    static INPUT: std::cell::RefCell<Option<iced::advanced::subscription::EventStream>> =
        const { std::cell::RefCell::new(None) };
}

/// The runtime events of the recipe starting now: `Some` once, inside
/// a native stream factory.
fn events() -> Option<iced::advanced::subscription::EventStream> {
    INPUT.with(|slot| slot.borrow_mut().take())
}

struct Bound<F, Message> {
    instance: application::Instance,
    generation: program::GenerationId,
    recipe: host::stream::Recipe,
    emit: F,
    output: std::marker::PhantomData<fn() -> Message>,
}

struct Failed<F, Message> {
    instance: application::Instance,
    generation: program::GenerationId,
    error: Error,
    emit: F,
    output: std::marker::PhantomData<fn() -> Message>,
}

impl<Message, F> iced::advanced::subscription::Recipe for Bound<F, Message>
where
    Message: Send + 'static,
    F: Fn(runtime::Message) -> Message + Send + 'static,
{
    type Output = Message;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        hash_identity(
            state,
            self.instance,
            self.generation,
            &self.recipe.identity(),
        );
    }

    fn stream(
        self: Box<Self>,
        input: iced::advanced::subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Self::Output> {
        use iced::futures::stream::StreamExt;

        let instance = self.instance;
        let generation = self.generation;
        let emit = self.emit;
        // A stream of runtime events (`keyboard::listen()`) takes iced's
        // event input as it starts.
        INPUT.with(|slot| *slot.borrow_mut() = Some(input));
        let issued = self.recipe.start();
        INPUT.with(|slot| slot.borrow_mut().take());
        Box::pin(issued.map(move |result| match result {
            Ok(payload) => emit(runtime::Message::subscription(
                instance, generation, payload,
            )),
            Err(error) => emit(runtime::Message::subscription_error(
                instance,
                generation,
                Error::Stream(error),
            )),
        }))
    }
}

fn hash_identity<T: Hash>(
    state: &mut iced::advanced::subscription::Hasher,
    instance: application::Instance,
    generation: program::GenerationId,
    identity: &T,
) {
    instance.hash(state);
    generation.hash(state);
    identity.hash(state);
}

impl<Message, F> iced::advanced::subscription::Recipe for Failed<F, Message>
where
    Message: Send + 'static,
    F: Fn(runtime::Message) -> Message + Send + 'static,
{
    type Output = Message;

    fn hash(&self, state: &mut iced::advanced::subscription::Hasher) {
        "rawr-subscription-lowering-failure-v1".hash(state);
        self.instance.hash(state);
        self.generation.hash(state);
        self.error.to_string().hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced::advanced::subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Self::Output> {
        let message = (self.emit)(runtime::Message::subscription_error(
            self.instance,
            self.generation,
            self.error,
        ));
        Box::pin(iced::futures::stream::once(async move { message }))
    }
}

pub(crate) fn bind(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    let builder = iced;
    bind_events(builder);
    builder
        .bind_with_context("iced::Subscription::batch", |call, args| {
            let [plans] = args else {
                return Err(arity("Subscription::batch", 1, args.len()));
            };
            let plans = plans.take().into_array().map_err(|actual| {
                EvalAltResult::ErrorRuntime(
                    format!("Subscription::batch expected a list, found `{actual}`").into(),
                    Position::NONE,
                )
            })?;
            let plans = plans
                .into_iter()
                .map(|mut plan| take(&mut plan, "Subscription::batch"))
                .collect::<Result<Vec<_>, _>>()?;
            host::stream::Plan::batch(plans, call)
                .map(Dynamic::from)
                .map_err(witness_error)
        })
        .expect("the iced facade must retain Subscription::batch");
    builder
        .bind_with_context("iced::Subscription::filter_map", |call, args| {
            if args.len() != 2 {
                return Err(arity("Subscription::filter_map", 2, args.len()));
            }
            let plan = take(args[0], "Subscription::filter_map")?;
            let mapper = args[1]
                .take()
                .try_cast_result::<item::Value>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!(
                            "Subscription::filter_map received `{}`, expected an exact symbolic mapper",
                            value.type_name()
                        )
                        .into(),
                        Position::NONE,
                    )
                })?;
            plan.filter_map(mapper, call)
                .map(Dynamic::from)
                .map_err(witness_error)
        })
        .expect("the iced facade must retain the exact Subscription::filter_map item");
    builder
        .bind_type_attestation("iced::Subscription", |value, expected| {
            let plan = value.read_lock::<host::stream::Plan>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(plan.witness())))
                .flatten()
        })
        .expect("the iced facade must declare generic Subscription exactly once");
    builder
        .bind_with_context("iced::Subscription::none", |call, args| {
            if !args.is_empty() {
                return Err(arity("Subscription::none", 0, args.len()));
            }
            host::stream::Plan::none(call)
                .map(Dynamic::from)
                .map_err(witness_error)
        })
        .expect("the iced facade must retain the exact Subscription::none item");
    rawr.bind_with_context("rawr_iced::subscription::batch", |call, args| {
        let mut plans = Vec::with_capacity(args.len());
        for value in args {
            plans.push(take(value, "Subscription::batch")?);
        }
        host::stream::Plan::batch(plans, call)
            .map(Dynamic::from)
            .map_err(witness_error)
    })
    .expect("the rawr_iced facade must retain the exact Subscription list builder");
    builder
        .bind_with_context("iced::Subscription::map", |call, args| {
            if args.len() != 2 {
                return Err(arity("Subscription::map", 2, args.len()));
            }
            let plan = take(args[0], "Subscription::map")?;
            let mapper = args[1]
                .take()
                .try_cast_result::<item::Value>()
                .map_err(|value| {
                    EvalAltResult::ErrorRuntime(
                        format!(
                            "Subscription::map received `{}`, expected an exact symbolic mapper",
                            value.type_name()
                        )
                        .into(),
                        Position::NONE,
                    )
                })?;
            plan.map(mapper, call)
                .map(Dynamic::from)
                .map_err(witness_error)
        })
        .expect("the iced facade must retain the exact Subscription::map item");
}

/// `event::listen()`, `event::listen_url()`, `keyboard::listen()`, `window::frames()` and
/// `time::every(duration)`: native streams of iced's runtime events, as
/// iced's own subscriptions filter them, and of timer ticks.
fn bind_events(builder: &mut host::runtime::Builder) {
    use iced::advanced::subscription::Event as Input;
    use iced::futures::stream::{self, StreamExt};

    type Native = iced::futures::stream::BoxStream<'static, Result<Dynamic, ()>>;

    fn input(filter: fn(Input) -> Option<Dynamic>) -> Native {
        match events() {
            Some(events) => events.filter_map(move |event| async move { filter(event).map(Ok) }).boxed(),
            None => stream::empty().boxed(),
        }
    }

    let facade = crate::facade().expect("the iced facade builds");
    let definition = |path: &str| {
        facade
            .items()
            .find(|item| item.path() == path)
            .cloned()
            .expect("the iced facade declares every event stream")
    };
    let no_arguments = |_: usize, _: &Dynamic, _: &mut host::stream::identity::Writer| {
        Err(host::stream::identity::Error::new("the stream takes no arguments"))
    };
    let unit = |(): ()| Dynamic::UNIT;
    for (path, filter) in [
        (
            "iced::event::listen",
            (|event| match event {
                Input::Interaction {
                    event: iced::Event::Window(iced::window::Event::RedrawRequested(_)),
                    ..
                } => None,
                Input::Interaction {
                    event,
                    status: iced::event::Status::Ignored,
                    ..
                } => Some(Dynamic::from(event)),
                _ => None,
            }) as fn(Input) -> Option<Dynamic>,
        ),
        ("iced::keyboard::listen", |event| match event {
            Input::Interaction {
                event: iced::Event::Keyboard(event),
                status: iced::event::Status::Ignored,
                ..
            } => Some(Dynamic::from(event)),
            _ => None,
        }),
        ("iced::event::listen_url", |event| match event {
            Input::PlatformSpecific(iced::advanced::subscription::PlatformSpecific::MacOS(
                iced::advanced::subscription::MacOS::ReceivedUrl(url),
            )) => Some(Dynamic::from(url)),
            _ => None,
        }),
        ("iced::window::frames", |event| match event {
            Input::Interaction {
                event: iced::Event::Window(iced::window::Event::RedrawRequested(now)),
                ..
            } => Some(Dynamic::from(crate::marker::Instant(now))),
            _ => None,
        }),
    ] {
        builder
            .bind_stream_operation(&definition(path), no_arguments, move |_| input(filter), unit)
            .expect("the iced facade must retain every event stream");
    }
    builder
        .bind_stream_operation(
            &definition("iced::time::every"),
            |index, value, writer| {
                let duration = value.read_lock::<std::time::Duration>().ok_or_else(|| {
                    host::stream::identity::Error::new("expected a `time::Duration`")
                })?;
                if index != 0 {
                    return Err(host::stream::identity::Error::new("unexpected argument"));
                }
                writer.u64(u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX));
                Ok(())
            },
            |arguments| {
                let duration = arguments
                    .get(0)
                    .and_then(|value| value.read_lock::<std::time::Duration>().map(|d| *d))
                    .unwrap_or(std::time::Duration::from_secs(1))
                    .max(std::time::Duration::from_millis(1));
                ticks(duration)
            },
            unit,
        )
        .expect("the iced facade must retain time::every");
}

/// A tick every `duration`, from a timer thread that ends with the stream.
fn ticks(
    duration: std::time::Duration,
) -> iced::futures::stream::BoxStream<'static, Result<Dynamic, ()>> {
    use iced::futures::StreamExt;

    let (mut sender, receiver) = iced::futures::channel::mpsc::channel(1);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(duration);
            match sender.try_send(()) {
                Ok(()) => {}
                Err(error) if error.is_full() => {}
                Err(_) => break,
            }
        }
    });
    receiver
        .map(|()| Ok(Dynamic::from(crate::marker::Instant::now())))
        .boxed()
}

fn take(value: &mut Dynamic, operation: &str) -> Result<host::stream::Plan, Box<EvalAltResult>> {
    value
        .take()
        .try_cast_result::<host::stream::Plan>()
        .map_err(|value| {
            EvalAltResult::ErrorRuntime(
                format!(
                    "{operation} received `{}`, expected a durable iced::Subscription plan",
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
            "native subscription binding `{operation}` received {actual} operand(s), expected {expected}"
        )
        .into(),
        Position::NONE,
    )
    .into()
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Representation { actual } => write!(
                formatter,
                "application callback returned `{actual}`, expected a durable iced::Subscription plan"
            ),
            Self::Stream(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Representation { .. } => None,
            Self::Stream(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hash_identity;
    use crate::program;
    use std::hash::Hasher as _;

    #[test]
    fn recipe_hash_changes_for_instance_generation_and_plan_identity() {
        let first = boot();
        let second = boot();
        let instance = first.instance();
        let other_instance = rawr::application::Instance::new();

        let baseline = hash(instance, first.generation(), 7_u64);
        assert_eq!(baseline, hash(instance, first.generation(), 7_u64));
        assert_ne!(baseline, hash(other_instance, first.generation(), 7_u64));
        assert_ne!(baseline, hash(instance, second.generation(), 7_u64));
        assert_ne!(baseline, hash(instance, first.generation(), 8_u64));
    }

    fn hash(
        instance: rawr::application::Instance,
        generation: rawr::program::GenerationId,
        identity: u64,
    ) -> u64 {
        let mut state = iced::advanced::subscription::Hasher::default();
        hash_identity(&mut state, instance, generation, &identity);
        state.finish()
    }

    fn boot() -> program::Adapter {
        crate::application::Compiler::new()
            .unwrap()
            .check_source(
                r#"
use iced::Element;
use iced::widget::text;
struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("identity") }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#,
            )
            .unwrap()
            .prepare()
            .unwrap()
            .into_adapter()
    }
}
