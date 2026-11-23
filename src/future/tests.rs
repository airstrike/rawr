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

    #[cfg(feature = "rust-import")]
    fn share(&self) -> Option<super::Program> {
        Some(crate::island::shared(
            self.authority.clone(),
            self.runtime.clone(),
        ))
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
        #[tokio::main]
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
        #[tokio::main]
        pub async fn main() -> i64 { boom().await + 1 }
        "#,
    );
    let error = block_on(future).unwrap_err();
    assert!(super::message(&error).contains("no luck"), "{error}");
}

/// A `move` closure `poll_fn` calls keeps what each call changed in what
/// it captured, as Rust's `FnMut` closure owns its captures.
#[test]
fn poll_fn_keeps_what_its_move_closure_changed() {
    let future = main_of(
        r#"
        #[tokio::main]
        pub async fn main() -> i64 {
            let mut n = 10;
            std::future::poll_fn(move |cx| {
                n += 1;
                if n < 13 {
                    cx.waker().wake_by_ref();
                    return std::task::Poll::Pending;
                }
                std::task::Poll::Ready(n)
            })
            .await
        }
        "#,
    );
    let mut script = future.cast::<super::Future>().into_script();
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..5 {
        if let Poll::Ready(output) = script.poll_output(&mut cx) {
            assert_eq!(output.unwrap().as_int().unwrap(), 13);
            return;
        }
    }
    panic!("the closure lost what it changed");
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
             #[tokio::main]
             pub async fn main() -> Result<(i64, i64), i64> { tokio::try_join!(one(), one()) }",
        )
        .expect_err("try_join! is refused");
    assert!(error.to_string().contains("try_join!"), "{error}");
}

/// Poll `script` to its end on this thread, parking between wakes.
#[cfg(feature = "rust-import")]
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
#[cfg(feature = "rust-import")]
#[derive(Debug)]
struct Token;

#[cfg(feature = "rust-import")]
static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "rust-import")]
static STEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "rust-import")]
impl Token {
    fn new() -> Self {
        LIVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

#[cfg(feature = "rust-import")]
impl Clone for Token {
    fn clone(&self) -> Self {
        Self::new()
    }
}

#[cfg(feature = "rust-import")]
impl Drop for Token {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A host for the owner-queue tests: `testing::step()` counts the steps that
/// ran, `testing::boom()` panics, `testing::pending()` is a future that never
/// ends, and `testing::token()` makes a counted value.
#[cfg(feature = "rust-import")]
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
        type_ref::Type::external(future.clone(), [type_ref::Type::unit()]).unwrap(),
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
#[cfg(feature = "rust-import")]
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
    super::serve(Rc::new(Program {
        authority: authority.clone(),
        runtime: registry.clone(),
    }));
    functions
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
        .collect()
}

/// Steps polled on other threads run there while no owner drains them, as
/// a script future iced's executor polls does (#1949). Once one does, they
/// queue for the owner, which drains them (D527). A future dropped before
/// the owner reaches its step runs no more, a step's panic reaches the
/// thread that polls it, and a body dropped on another thread drops what
/// it holds on the owner. One test, since the owner's queue is the
/// process's.
#[cfg(feature = "rust-import")]
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
        &["cancelled", "panicking", "held", "joined", "joined"],
    )
    .into_iter();
    let (cancelled, panicking, held, joined, alone) = (
        futures.next().unwrap(),
        futures.next().unwrap(),
        futures.next().unwrap(),
        futures.next().unwrap(),
        futures.next().unwrap(),
    );
    // No owner drains steps yet: each runs on the thread that polls it.
    let script = alone.into_script();
    let alone = std::thread::spawn(move || block_on_here(script)).join();
    assert_eq!(alone.unwrap().into_inner().as_int().unwrap(), 10);
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

/// An async fn's future holds each `&mut` parameter, its receiver's too,
/// as the caller's place across its awaits (#176): a `Vec` and a `String`,
/// a program struct's fields, a method's `&mut self` beside a `&mut`
/// parameter, a borrow passed on to another async fn, a `&mut` local an
/// async block captures, and a closure inside the body. Each place is
/// borrowed again after its future ends, by an async fn, a method and a
/// plain fn. The text is what rustc's build prints.
#[test]
fn an_async_fns_mut_parameters_are_the_callers_places_across_awaits() {
    let future = main_of(
        r#"
        struct Bag { items: Vec<i64>, name: String }
        async fn tick() {}
        async fn push(items: &mut Vec<i64>) { tick().await; items.push(4); }
        async fn both(a: &mut Vec<i64>, b: &mut String) { a.push(2); tick().await; b.push_str("x"); }
        async fn pass(a: &mut Vec<i64>, b: &mut String) { both(a, b).await; push(a).await; }
        async fn adds(items: &mut Vec<i64>) {
            tick().await;
            let mut add = |x| items.push(x);
            add(5);
            add(6);
        }
        fn grow(items: &mut Vec<i64>) { let mut add = |x| items.push(x); add(9); }
        impl Bag {
            async fn fill(&mut self, other: &mut Vec<i64>) {
                tick().await;
                self.items.push(7);
                other.push(8);
            }
            fn plain(&mut self) { self.items.push(5); }
        }
        #[tokio::main]
        pub async fn main() -> String {
            let mut bag = Bag { items: vec![1], name: String::from("n") };
            let mut v = vec![0];
            both(&mut bag.items, &mut bag.name).await;
            let f = push(&mut bag.items);
            f.await;
            bag.fill(&mut v).await;
            bag.fill(&mut v).await;
            bag.plain();
            pass(&mut v, &mut bag.name).await;
            let mut i = 0;
            while i < 2 { push(&mut v).await; i += 1; }
            grow(&mut v);
            let r = &mut v;
            let g = async move { r.push(3); tick().await; push(r).await; };
            g.await;
            adds(&mut v).await;
            format!("{:?} {} {v:?}", bag.items, bag.name)
        }
        "#,
    );
    assert_eq!(
        block_on(future).unwrap().into_string().unwrap(),
        "[1, 2, 4, 7, 7, 5] nxx [0, 8, 8, 2, 4, 4, 4, 9, 3, 4, 5, 6]"
    );
}

/// A `&mut` local given new borrows in an async body, of a whole local, of
/// a field and in a loop, across awaits: each write reaches the place it
/// borrows then. The text is what rustc's build prints (#123).
#[test]
fn an_async_bodys_mut_local_takes_new_borrows_across_awaits() {
    let future = main_of(
        r#"
        struct Pair { left: Vec<i64>, right: Vec<i64> }
        async fn tick() {}
        #[tokio::main]
        pub async fn main() -> String {
            let mut a = 1;
            let mut b = 2;
            let mut r = &mut a;
            *r += 10;
            tick().await;
            r = &mut b;
            *r += 20;
            tick().await;
            *r += 300;
            let mut p = Pair { left: vec![1], right: vec![2] };
            let mut side = &mut p.left;
            side.push(3);
            tick().await;
            side = &mut p.right;
            side.push(4);
            let mut x = 0;
            let mut y = 0;
            let mut at = &mut x;
            let mut i = 0;
            while i < 4 {
                *at += i;
                tick().await;
                if i % 2 == 0 {
                    at = &mut y;
                } else {
                    at = &mut x;
                }
                i += 1;
            }
            format!("{a} {b} {:?} {:?} {x} {y}", p.left, p.right)
        }
        "#,
    );
    assert_eq!(
        block_on(future).unwrap().into_string().unwrap(),
        "11 322 [1, 3] [2, 4] 2 4"
    );
}

/// A use of a place while a future that borrows it lives is refused with
/// rustc's error: an async fn's future holds what its inputs borrow, as
/// an `impl Trait` output does, and a use once it has ended is accepted.
#[test]
fn a_use_of_a_place_a_live_future_borrows_is_rustcs_error() {
    let compiled = |body: &str| {
        let mut engine = Engine::new();
        engine
            .register_host_facade(super::facade::definition().unwrap())
            .unwrap();
        let source = format!(
            "async fn push(items: &mut Vec<i64>) {{ items.push(4); }}
             async fn len(items: &Vec<i64>) -> usize {{ items.len() }}
             struct C {{ n: i64 }}
             impl C {{ async fn bump(&mut self) {{ self.n += 1; }} }}
             #[tokio::main] pub async fn main() {{ {body} }}"
        );
        Shared::new(engine)
            .compile_authority(&source)
            .err()
            .map(|error| error.to_string())
    };
    for (body, code) in [
        (
            "let mut v = vec![1]; let f = push(&mut v); v.push(2); f.await;",
            "E0499",
        ),
        (
            "let mut v = vec![1]; let f = push(&mut v); let n = v.len(); f.await;",
            "E0502",
        ),
        (
            "let mut v = vec![1]; let f = len(&v); v.push(2); f.await;",
            "E0502",
        ),
        (
            "let mut c = C { n: 0 }; let f = c.bump(); c.n += 1; f.await;",
            "E0503",
        ),
        (
            "let mut c = C { n: 0 }; let f = c.bump(); let n = c.n; f.await;",
            "E0503",
        ),
    ] {
        let error = compiled(body).unwrap_or_else(|| panic!("accepted: {body}"));
        assert!(error.contains(code), "{body}: {error}");
    }
    assert_eq!(
        compiled("let mut v = vec![1]; let f = push(&mut v); f.await; v.push(2);"),
        None
    );
}

/// A write or a move of a place while a future that borrows it lives is
/// rustc's E0506 or E0505, at its span: a program's `async fn`'s future,
/// and an imported one's, which keeps each input it lends (D683.2). Once
/// the future has ended, both are accepted.
#[cfg(feature = "rust-import")]
#[test]
fn a_write_or_a_move_of_a_place_a_live_future_leases_is_rustcs_error() {
    use host::{facade, item, signature, type_ref};
    let core = super::facade::definition().unwrap();
    let future = core.export_type("future::Future").unwrap();
    let mut builder = facade::Builder::new("leasing", facade::Abi::new(1).unwrap()).unwrap();
    builder.type_::<Token>("Token", &[], &[]).unwrap();
    let token = || type_ref::Type::registered("Token").unwrap();
    let mut change = signature::Builder::new();
    change.parameter(type_ref::Type::reference(true, token()));
    change.output(type_ref::Type::external(future, [type_ref::Type::unit()]).unwrap());
    builder
        .item(
            item::Definition::function("change", change.finish())
                .unwrap()
                .imported(rawr_import::surface::identity::Selector::Declaration)
                .lending(1),
        )
        .unwrap();
    let mut made = signature::Builder::new();
    made.output(token());
    builder
        .item(item::Definition::function("token", made.finish()).unwrap())
        .unwrap();
    let definition = builder.finish().unwrap();
    let compiled = |body: &str| {
        let mut engine = Engine::new();
        engine
            .register_host_facade(super::facade::definition().unwrap())
            .unwrap();
        engine.register_host_facade(definition.clone()).unwrap();
        let source = format!(
            "async fn len(items: &Vec<i64>) -> usize {{ items.len() }}
             #[tokio::main] pub async fn main() {{ {body} }}"
        );
        Shared::new(engine)
            .compile_authority(&source)
            .err()
            .map(|error| error.to_string())
    };
    let leased = "let mut t = leasing::token(); let f = leasing::change(&mut t);";
    for (body, error) in [
        (
            "let mut v = vec![1]; let f = len(&v); v = vec![2]; f.await;".to_string(),
            "cannot assign to `v` because it is borrowed (E0506)",
        ),
        (
            "let v = vec![1]; let f = len(&v); let w = v; f.await;".to_string(),
            "cannot move out of `v` because it is borrowed (E0505)",
        ),
        (
            format!("{leased} t = leasing::token(); f.await;"),
            "cannot assign to `t` because it is borrowed (E0506)",
        ),
        (
            format!("{leased} let u = t; f.await;"),
            "cannot move out of `t` because it is borrowed (E0505)",
        ),
        (
            format!("{leased} let r = &t; f.await;"),
            "cannot borrow `t` as immutable because it is also borrowed as mutable (E0502)",
        ),
    ] {
        let report = compiled(&body).unwrap_or_else(|| panic!("accepted: {body}"));
        assert!(report.starts_with(error), "{body}: {report}");
    }
    for body in [
        "let mut v = vec![1]; let f = len(&v); f.await; v = vec![2]; let w = v;".to_string(),
        format!("{leased} f.await; t = leasing::token(); let u = t;"),
    ] {
        assert_eq!(compiled(&body), None, "{body}");
    }
}
