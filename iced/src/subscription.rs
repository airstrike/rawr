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
        _input: iced::advanced::subscription::EventStream,
    ) -> iced::futures::stream::BoxStream<'static, Self::Output> {
        use iced::futures::stream::StreamExt;

        let instance = self.instance;
        let generation = self.generation;
        let emit = self.emit;
        Box::pin(self.recipe.start().map(move |result| match result {
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
