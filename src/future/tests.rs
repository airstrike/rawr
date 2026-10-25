//! Async code in a program without iced: it compiles against the core
//! future and runs as a std future (D511.3, D513.3).

use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::{host, item, Dynamic, Engine, RawrResultOf, Shared};

struct Program {
    authority: crate::program::Authority,
    runtime: Shared<host::runtime::Registry>,
}

impl super::Host for Program {
    fn invoke(&self, function: &item::Value, arguments: &mut [Dynamic]) -> RawrResultOf<Dynamic> {
        self.authority
            .invoke_bound(function, arguments, self.runtime.clone())
            .map_err(|error| super::error(error.to_string()))
    }
}

/// Compile `source` with the core future, and call its `main`.
fn main_of(source: &str) -> Dynamic {
    let mut engine = Engine::new();
    engine
        .register_host_facade(super::facade::definition().unwrap())
        .unwrap();
    let engine = Shared::new(engine);
    let authority = engine
        .compile_authority(source)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut registry = host::runtime::Registry::new();
    registry.install(super::facade::runtime().unwrap()).unwrap();
    let program = Program {
        authority,
        runtime: Shared::new(registry),
    };
    let main = program
        .authority
        .items()
        .find(|item| item.key().name() == "main")
        .expect("a main")
        .clone();
    let future = program
        .authority
        .invoke_bound(&main, &mut [], program.runtime.clone())
        .unwrap_or_else(|error| panic!("{error}"));
    super::serve(Rc::new(program));
    future
}

fn block_on(future: Dynamic) -> RawrResultOf<Dynamic> {
    let mut script = future.cast::<super::Future>().into_script();
    let mut cx = Context::from_waker(Waker::noop());
    match script.poll_output(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("no native future waits here"),
    }
}

#[test]
fn an_async_main_awaits_async_fns_and_joins_them() {
    let future = main_of(
        r#"
        async fn add(a: i64, b: i64) -> i64 { a + b }
        async fn twice(n: i64) -> i64 { let m = add(n, n).await; m }
        pub async fn main() -> i64 {
            let (x, y) = tokio::join!(twice(1), add(3, 4));
            let z = async move { x * 10 }.await;
            z + y
        }
        "#,
    );
    assert_eq!(block_on(future).unwrap().as_int().unwrap(), 27);
}

#[test]
fn a_step_that_fails_ends_the_future_with_its_message() {
    let future = main_of(
        r#"
        async fn boom() -> i64 { panic!("no luck") }
        pub async fn main() -> i64 { boom().await + 1 }
        "#,
    );
    let error = block_on(future).unwrap_err();
    assert!(super::message(&error).contains("no luck"), "{error}");
}

#[test]
fn try_join_is_not_yet_in_a_program_without_iced() {
    let mut engine = Engine::new();
    engine
        .register_host_facade(super::facade::definition().unwrap())
        .unwrap();
    let error = Shared::new(engine)
        .compile_authority(
            "async fn one() -> Result<i64, i64> { Ok(1) }
             pub async fn main() -> Result<(i64, i64), i64> { tokio::try_join!(one(), one()) }",
        )
        .expect_err("try_join! is refused");
    assert!(error.to_string().contains("try_join!"), "{error}");
}

/// Poll `script` to its end on this thread, parking between wakes.
#[cfg(feature = "sync")]
fn block_on_here(script: super::Script) -> crate::Value {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(std::sync::Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut script = std::pin::pin!(script);
    loop {
        if let Poll::Ready(value) = std::future::Future::poll(script.as_mut(), &mut cx) {
            return value;
        }
        std::thread::park();
    }
}

/// A value whose copies are counted while they live.
#[cfg(feature = "sync")]
#[derive(Debug)]
struct Token;

#[cfg(feature = "sync")]
static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "sync")]
static STEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "sync")]
impl Token {
    fn new() -> Self {
        LIVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

#[cfg(feature = "sync")]
impl Clone for Token {
    fn clone(&self) -> Self {
        Self::new()
    }
}

#[cfg(feature = "sync")]
impl Drop for Token {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A host for the owner-queue tests: `testing::step()` counts the steps that
/// ran, `testing::boom()` panics, `testing::pending()` is a future that never
/// ends, and `testing::token()` makes a counted value.
#[cfg(feature = "sync")]
fn testing() -> (host::facade::Definition, host::runtime::Facade) {
    use host::{facade, item, signature, type_ref};
    let core = super::facade::definition().unwrap();
    let future = core.export_type("future::Future").unwrap();
    let mut builder = facade::Builder::new("testing", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Token>("Token", &[], &[]).unwrap();
    let function = |builder: &mut facade::Builder, path: &str, output: type_ref::Type| {
        let mut signature = signature::Builder::new();
        signature.output(output);
        builder
            .item(item::Definition::function(path, signature.finish()).unwrap())
            .unwrap();
    };
    function(&mut builder, "step", type_ref::Type::unit());
    function(&mut builder, "boom", type_ref::Type::unit());
    function(
        &mut builder,
        "pending",
        type_ref::Type::external(future, [type_ref::Type::unit()]).unwrap(),
    );
    function(
        &mut builder,
        "token",
        type_ref::Type::registered("Token").unwrap(),
    );
    let definition = builder.finish().unwrap();
    let mut runtime = host::runtime::Builder::new(definition.clone());
    runtime
        .bind_with_context("testing::step", |_, _| {
            STEPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Dynamic::UNIT)
        })
        .unwrap();
    runtime
        .bind_with_context("testing::boom", |_, _| panic!("boom in a step"))
        .unwrap();
    runtime
        .bind_with_context("testing::pending", |_, _| {
            Ok(Dynamic::from(super::Future::native(|| {
                Box::pin(std::future::pending::<super::Finish>())
            })))
        })
        .unwrap();
    runtime
        .bind_with_context("testing::token", |_, _| Ok(Dynamic::from(Token::new())))
        .unwrap();
    (definition, runtime.finish())
}

/// The futures of `functions` in `source`, compiled with the core future and
/// the testing host, whose program this thread then owns.
#[cfg(feature = "sync")]
fn futures_with_testing(source: &str, functions: &[&str]) -> Vec<super::Future> {
    let (definition, runtime) = testing();
    let mut engine = Engine::new();
    engine
        .register_host_facade(super::facade::definition().unwrap())
        .unwrap();
    engine.register_host_facade(definition).unwrap();
    let engine = Shared::new(engine);
    let mut registry = host::runtime::Registry::new();
    registry.install(super::facade::runtime().unwrap()).unwrap();
    registry.install(runtime).unwrap();
    let registry = Shared::new(registry);
    let authority = engine
        .compile_authority(source)
        .unwrap_or_else(|error| panic!("{error}"));
    let futures = functions
        .iter()
        .map(|name| {
            let function = authority
                .items()
                .find(|item| item.key().name() == *name)
                .expect("the function")
                .clone();
            authority
                .invoke_bound(&function, &mut [], registry.clone())
                .unwrap_or_else(|error| panic!("{error}"))
                .cast::<super::Future>()
        })
        .collect();
    super::serve(Rc::new(Program {
        authority,
        runtime: registry,
    }));
    futures
}

/// Steps polled on other threads queue for the owner, which drains them
/// (D527). A future dropped before the owner reaches its step runs no more,
/// a step's panic reaches the thread that polls it, and a body dropped on
/// another thread drops what it holds on the owner. One test, since the
/// owner's queue is the process's.
#[cfg(feature = "sync")]
#[test]
fn steps_polled_elsewhere_queue_for_the_owner_which_drains_them() {
    use std::sync::atomic::Ordering::SeqCst;
    let mut futures = futures_with_testing(
        "pub async fn cancelled() -> i64 { testing::step(); 1 }
         pub async fn panicking() -> i64 { testing::boom(); 1 }
         pub async fn held() -> i64 {
             for _token in [testing::token(), testing::token()] { testing::pending().await; }
             0
         }
         async fn add(a: i64, b: i64) -> i64 { a + b }
         pub async fn joined() -> i64 {
             let (x, y) = tokio::join!(add(1, 2), add(3, 4));
             add(x, y).await
         }",
        &["cancelled", "panicking", "held", "joined"],
    )
    .into_iter();
    let (cancelled, panicking, held, joined) = (
        futures.next().unwrap(),
        futures.next().unwrap(),
        futures.next().unwrap(),
        futures.next().unwrap(),
    );
    super::drive(Waker::noop());
    let off_owner = |future: super::Future, polls: usize| {
        std::thread::spawn(move || {
            assert!(!super::owns());
            let mut script = future.into_script();
            let mut cx = Context::from_waker(Waker::noop());
            for _ in 0..polls {
                let _ = std::future::Future::poll(std::pin::Pin::new(&mut script), &mut cx);
            }
            script
        })
        .join()
    };

    // Polled once elsewhere, then dropped there: its queued step never runs.
    drop(off_owner(cancelled, 1).unwrap());
    super::drain();
    assert_eq!(STEPS.load(SeqCst), 0);

    // A step that panics on the owner panics its poller with the payload.
    let script = off_owner(panicking, 1).unwrap();
    super::drain();
    let panicked = std::thread::spawn(move || {
        let mut script = script;
        let mut cx = Context::from_waker(Waker::noop());
        std::future::Future::poll(std::pin::Pin::new(&mut script), &mut cx)
    })
    .join()
    .expect_err("the poller panics");
    assert_eq!(panicked.downcast_ref::<&str>(), Some(&"boom in a step"));

    // Suspended in a `for` loop, then dropped on another thread: its
    // iterator, and the token left in it, drop on the owner.
    let script = off_owner(held, 1).unwrap();
    super::drain();
    let script = std::thread::spawn(move || {
        let mut script = script;
        let mut cx = Context::from_waker(Waker::noop());
        assert!(std::future::Future::poll(std::pin::Pin::new(&mut script), &mut cx).is_pending());
        script
    })
    .join()
    .unwrap();
    assert_eq!(LIVE.load(SeqCst), 2);
    std::thread::spawn(move || drop(script)).join().unwrap();
    super::drain();
    assert_eq!(LIVE.load(SeqCst), 0);

    // A whole program polled elsewhere runs each step on the owner.
    let script = joined.into_script();
    let poller = std::thread::spawn(move || block_on_here(script));
    while !poller.is_finished() {
        super::drain();
        std::thread::yield_now();
    }
    super::drain();
    assert_eq!(poller.join().unwrap().into_inner().as_int().unwrap(), 10);
}
