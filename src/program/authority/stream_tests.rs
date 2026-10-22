//! Stream authority regressions.

use super::Authority;
use crate::{host, Dynamic};
use crate::{Engine, Shared};
use futures_core::Stream;
use std::any::TypeId;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

struct Values(VecDeque<Result<Dynamic, ()>>);

impl Stream for Values {
    type Item = Result<Dynamic, ()>;

    fn poll_next(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop_front())
    }
}

#[test]
fn invalid_input_and_missing_mapper_preflight_before_identity_or_start() {
    let (engine, facade) = compiler();
    let issuer = authority(engine.clone(), "pub fn mapped(value: i64) -> bool { true }");
    let mapper = item(&issuer, "mapped");
    let encodes = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let runtime = runtime(
        &facade,
        Dynamic::from(1_i64),
        encodes.clone(),
        starts.clone(),
    );

    let invalid = request(&facade, Dynamic::from(true), mapper.clone(), bool_type());
    assert!(matches!(
        issuer.bind_stream(invalid, runtime.clone()),
        Err(host::stream::Error::Input { index: 0, .. })
    ));
    assert_eq!(encodes.load(Ordering::SeqCst), 0);
    assert_eq!(starts.load(Ordering::SeqCst), 0);

    let current = authority(engine, "pub fn other(value: i64) -> bool { true }");
    let stale = request(&facade, Dynamic::from(1_i64), mapper, bool_type());
    assert!(matches!(
        current.bind_stream(stale, runtime),
        Err(host::stream::Error::Mapper(_))
    ));
    assert_eq!(encodes.load(Ordering::SeqCst), 0);
    assert_eq!(starts.load(Ordering::SeqCst), 0);
}

#[test]
fn recipe_identity_is_stable_and_factory_start_is_deferred() {
    let (engine, facade) = compiler();
    let authority = authority(engine, "pub fn mapped(value: i64) -> bool { true }");
    let mapper = item(&authority, "mapped");
    let encodes = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let runtime = runtime(
        &facade,
        Dynamic::from(1_i64),
        encodes.clone(),
        starts.clone(),
    );

    let first = authority
        .bind_stream(
            request(&facade, Dynamic::from(7_i64), mapper.clone(), bool_type()),
            runtime.clone(),
        )
        .unwrap();
    let repeated = authority
        .bind_stream(
            request(&facade, Dynamic::from(7_i64), mapper.clone(), bool_type()),
            runtime.clone(),
        )
        .unwrap();
    let changed = authority
        .bind_stream(
            request(&facade, Dynamic::from(8_i64), mapper, bool_type()),
            runtime,
        )
        .unwrap();

    assert_eq!(first.identity(), repeated.identity());
    assert_ne!(first.identity(), changed.identity());
    assert_eq!(encodes.load(Ordering::SeqCst), 3);
    assert_eq!(starts.load(Ordering::SeqCst), 0);

    let _issued = first.start();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[test]
fn output_mapper_and_message_validation_remain_under_issuing_authority() {
    let (engine, facade) = compiler();
    let authority = authority(
        engine,
        r#"
pub fn boolean(value: i64) -> bool { true }
pub fn string(value: i64) -> String { "mapped" }
pub fn failing(value: i64) -> bool { throw "mapper failed"; }
"#,
    );
    let starts = Arc::new(AtomicUsize::new(0));
    let encodes = Arc::new(AtomicUsize::new(0));

    let invalid_output = runtime(
        &facade,
        Dynamic::from(true),
        encodes.clone(),
        starts.clone(),
    );
    let recipe = authority
        .bind_stream(
            request(
                &facade,
                Dynamic::from(1_i64),
                item(&authority, "boolean"),
                bool_type(),
            ),
            invalid_output,
        )
        .unwrap();
    assert!(matches!(
        poll_one(recipe.start()),
        Err(host::stream::Error::Output { .. })
    ));

    let invalid_message = runtime(
        &facade,
        Dynamic::from(1_i64),
        encodes.clone(),
        starts.clone(),
    );
    let recipe = authority
        .bind_stream(
            request(
                &facade,
                Dynamic::from(1_i64),
                item(&authority, "string"),
                bool_type(),
            ),
            invalid_message,
        )
        .unwrap();
    assert!(matches!(
        poll_one(recipe.start()),
        Err(host::stream::Error::Message { .. })
    ));

    let failing_mapper = runtime(
        &facade,
        Dynamic::from(1_i64),
        encodes.clone(),
        starts.clone(),
    );
    let recipe = authority
        .bind_stream(
            request(
                &facade,
                Dynamic::from(1_i64),
                item(&authority, "failing"),
                bool_type(),
            ),
            failing_mapper,
        )
        .unwrap();
    assert!(matches!(
        poll_one(recipe.start()),
        Err(host::stream::Error::Mapper(_))
    ));

    let valid = runtime(&facade, Dynamic::from(1_i64), encodes, starts.clone());
    let recipe = authority
        .bind_stream(
            request(
                &facade,
                Dynamic::from(1_i64),
                item(&authority, "boolean"),
                bool_type(),
            ),
            valid,
        )
        .unwrap();
    assert!(poll_one(recipe.start()).unwrap().as_bool().unwrap());
    assert_eq!(starts.load(Ordering::SeqCst), 4);
}

fn compiler() -> (Shared<Engine>, host::facade::Definition) {
    let facade = facade();
    let mut engine = Engine::new();
    engine.register_host_facade(facade.clone()).unwrap();
    (Shared::new(engine), facade)
}

fn authority(engine: Shared<Engine>, source: &str) -> Authority {
    let executable = engine.compile_executable(source).unwrap();
    Authority::new(engine, executable).unwrap()
}

fn item(authority: &Authority, name: &str) -> crate::item::Value {
    authority
        .items()
        .find(|item| item.key().name() == name)
        .unwrap()
        .clone()
}

fn facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
    builder.stream_plan_type("Subscription", "Message").unwrap();
    let mut signature = host::signature::Builder::new();
    signature
        .parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64))
        .output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(
            host::item::Definition::stream_operation(
                "ticks",
                signature.finish(),
                host::stream::Version::new(4),
            )
            .unwrap(),
        )
        .unwrap();
    builder.finish().unwrap()
}

fn runtime(
    facade: &host::facade::Definition,
    output: Dynamic,
    encodes: Arc<AtomicUsize>,
    starts: Arc<AtomicUsize>,
) -> Shared<host::runtime::Registry> {
    let selected = facade.items().next().unwrap().clone();
    let mut builder = host::runtime::Builder::new(facade.clone());
    builder
        .bind_stream_operation(
            &selected,
            move |_index, value, writer| {
                encodes.fetch_add(1, Ordering::SeqCst);
                writer.i64(value.as_int().map_err(host::stream::identity::Error::new)?);
                Ok(())
            },
            move |_arguments| {
                starts.fetch_add(1, Ordering::SeqCst);
                Values(VecDeque::from([Ok(output.clone())]))
            },
            |_| Dynamic::from(-1_i64),
        )
        .unwrap();
    let mut registry = host::runtime::Registry::new();
    registry.install(builder.finish()).unwrap();
    Shared::new(registry)
}

fn request(
    facade: &host::facade::Definition,
    argument: Dynamic,
    mapper: crate::item::Value,
    message: host::runtime::type_ref::Type,
) -> host::stream::Request {
    let definition = facade.items().next().unwrap();
    let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
    let proof = host::stream::Proof::new(
        owner,
        definition,
        vec![int_type()].into_boxed_slice(),
        int_type(),
    )
    .unwrap();
    host::stream::Request::new(
        host::stream::Call::seal(proof, vec![argument].into_boxed_slice()),
        vec![mapper].into_boxed_slice(),
        message,
    )
}

fn int_type() -> host::runtime::type_ref::Type {
    host::runtime::type_ref::Type::builtin("i64", TypeId::of::<crate::INT>())
}

fn bool_type() -> host::runtime::type_ref::Type {
    host::runtime::type_ref::Type::builtin("bool", TypeId::of::<bool>())
}

fn poll_one(mut issued: host::stream::Issued) -> Result<Dynamic, host::stream::Error> {
    let mut context = Context::from_waker(Waker::noop());
    match Pin::new(&mut issued).poll_next(&mut context) {
        Poll::Ready(Some(value)) => value,
        Poll::Ready(None) => panic!("bound stream ended before its first item"),
        Poll::Pending => panic!("test stream unexpectedly remained pending"),
    }
}
