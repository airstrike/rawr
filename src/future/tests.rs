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

#[cfg(feature = "sync")]
#[test]
fn a_script_polled_on_another_thread_runs_its_steps_on_the_owner() {
    let future = main_of(
        r#"
        async fn add(a: i64, b: i64) -> i64 { a + b }
        pub async fn main() -> i64 {
            let (x, y) = tokio::join!(add(1, 2), add(3, 4));
            add(x, y).await
        }
        "#,
    );
    let (jobs, queue) = std::sync::mpsc::channel::<super::Job>();
    super::forward(move |job| {
        let _ = jobs.send(job);
    });
    let script = future.cast::<super::Future>().into_script();
    let poller = std::thread::spawn(move || {
        assert!(!super::owns());
        block_on_here(script)
    });
    let mut served = 0;
    while !poller.is_finished() {
        if let Ok(job) = queue.recv_timeout(std::time::Duration::from_millis(5)) {
            job.run();
            served += 1;
        }
    }
    let value = poller.join().unwrap().into_inner();
    assert_eq!(value.as_int().unwrap(), 10);
    // Each async body's steps reached the owner as jobs.
    assert!(served >= 3, "{served}");
}
