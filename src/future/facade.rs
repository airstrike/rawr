//! The core future type as Rawr code names it (D511.3): `async` blocks and
//! `async fn`s in a program without iced give a `rawr::future::Future<T>`,
//! and so do native async functions in the plain runner. `join!` is
//! `rawr::future::__join{N}`. A host whose own type runs futures, as
//! rawr-iced's `Task` does, takes precedence where it is installed.
//!
//! It also runs what code needs to poll a future by hand (#1152): std's
//! `task::Context`, `Waker` and `Poll<T>`, `future::poll_fn`, and one poll
//! of a core future, which `Future::poll` is (`resolve::std_reexport` sends
//! the std paths here). A `Poll<T>` is an `Option<T>` at runtime.

use crate::capability;
use crate::host::{self, facade, item, signature, type_ref};
use crate::{Dynamic, EvalAltResult, Position};

/// The facade's root.
pub const ROOT: &str = "rawr";
/// The future type, qualified.
pub const FUTURE: &str = "rawr::future::Future";
/// The function an async body's step function is passed to.
pub const SCRIPT: &str = "rawr::future::script";
/// The function `.await` on a crate value whose type is a future passes
/// the value to first, relative to the facade's root.
const AWAITED: &str = "future::__awaited";
/// That function, qualified.
pub const AWAITED_PATH: &str = "rawr::future::__awaited";
/// The type of the context a future is polled with, std's `task::Context`.
pub const CONTEXT: &str = "rawr::future::Context";
/// std's `task::Waker`, Rust's own at runtime.
pub const WAKER: &str = "rawr::future::Waker";
/// std's `task::Poll<T>`, an `Option<T>` at runtime: `Some` is `Ready`.
pub const POLL: &str = "rawr::future::Poll";
/// std's `future::poll_fn`.
pub const POLL_FN: &str = "rawr::future::poll_fn";
/// One poll of a future by hand, `Future::poll`.
pub const POLL_ONCE: &str = "rawr::future::__poll";
/// The std definitions the facade runs, and its item for each, which a
/// path to the std item names (`resolve::std_reexport`).
pub const STD: [(&str, &str); 4] = [
    ("core::task::wake::Context", CONTEXT),
    ("core::task::wake::Waker", WAKER),
    ("core::task::poll::Poll", POLL),
    ("core::future::poll_fn::poll_fn", POLL_FN),
];
/// The most futures `join!` takes.
pub const MOST_FUTURES: usize = 5;

/// The `join!` function for `count` futures.
#[must_use]
pub fn join_path(count: usize) -> String {
    format!("rawr::future::__join{count}")
}

/// An engine for a program whose futures are the core's: Rawr's own, with
/// this facade installed.
#[must_use]
pub fn engine() -> crate::Engine {
    let mut engine = crate::Engine::new();
    engine
        .register_host_facade(definition().expect("the core future bindings are valid"))
        .expect("an engine has no other `rawr` bindings");
    engine
}

/// The facade's compile-time definition.
pub fn definition() -> Result<facade::Definition, host::Error> {
    use type_ref::Type;
    let mut builder = facade::Builder::new(ROOT, facade::Abi::new(1)?)?;
    builder.module("future")?;
    builder.type_::<super::Future>("future::Future", &["T"], &[])?;
    let future = |output: Type| Type::applied("future::Future", [output]);
    builder.type_::<super::Waking>("future::Context", &[], &[])?;
    builder.type_::<std::task::Waker>(
        "future::Waker",
        &[],
        &[capability::Kind::Clone, capability::Kind::Send],
    )?;
    let waker = || Type::registered("future::Waker");
    let mut waker_of = signature::Builder::new();
    waker_of.output(Type::reference(false, waker()?));
    builder.item(item::Definition::method(
        "future::Context::waker",
        crate::receiver::Mode::Shared,
        waker_of.finish(),
    )?)?;
    for (wake, receiver) in [
        ("wake", crate::receiver::Mode::Owned),
        ("wake_by_ref", crate::receiver::Mode::Shared),
    ] {
        builder.item(item::Definition::method(
            format!("future::Waker::{wake}"),
            receiver,
            signature::Builder::new().finish(),
        )?)?;
    }
    builder.type_::<crate::option::Value>("future::Poll", &["T"], &[])?;
    let poll = |output: Type| Type::applied("future::Poll", [output]);
    let context = || Type::registered("future::Context").map(|ty| Type::reference(true, ty));

    // `poll_fn(f)`: the future whose every poll calls `f` with the
    // polling task's context.
    let mut poll_fn = signature::Builder::new();
    poll_fn.type_parameter("T", &[])?;
    poll_fn.parameter(Type::callable(context()?, poll(Type::parameter("T")?)?));
    poll_fn.output(future(Type::parameter("T")?)?);
    builder.item(item::Definition::function(
        "future::poll_fn",
        poll_fn.finish(),
    )?)?;

    // `Future::poll(future, cx)` of a future the checker typed: one poll,
    // whose `Poll` of its output the checker gives it.
    let mut poll_once = signature::Builder::new();
    poll_once.type_parameter("F", &[])?;
    poll_once.type_parameter("T", &[])?;
    poll_once.parameter(Type::parameter("F")?);
    poll_once.parameter(context()?);
    poll_once.output(poll(Type::parameter("T")?)?);
    builder.item(item::Definition::function(
        "future::__poll",
        poll_once.finish(),
    )?)?;

    for state in ["is_ready", "is_pending"] {
        let mut method = signature::Builder::new();
        method.output(Type::builtin(type_ref::Builtin::Bool));
        builder.item(item::Definition::method(
            format!("future::Poll::{state}"),
            crate::receiver::Mode::Shared,
            method.finish(),
        )?)?;
    }

    let mut script = signature::Builder::new();
    script.type_parameter("Resume", &[])?;
    script.type_parameter("Output", &[])?;
    script.parameter(Type::callable(
        Type::parameter("Resume")?,
        Type::parameter("Output")?,
    ));
    script.output(future(Type::parameter("Output")?)?);
    builder.item(item::Definition::function(
        "future::script",
        script.finish(),
    )?)?;

    // `.await` on a crate value whose type is a future, as tokio's
    // `JoinHandle<T>`: the core's future of its `Output`.
    let mut awaited = signature::Builder::new();
    awaited.type_parameter("Value", &[])?;
    awaited.type_parameter("Output", &[])?;
    awaited.parameter(Type::parameter("Value")?);
    awaited.output(future(Type::parameter("Output")?)?);
    builder.item(item::Definition::function(AWAITED, awaited.finish())?)?;

    const NAMES: [&str; MOST_FUTURES] = ["A", "B", "C", "D", "F"];
    for count in 2..=MOST_FUTURES {
        let names = &NAMES[..count];
        let mut join = signature::Builder::new();
        let mut outputs = Vec::with_capacity(count);
        for name in names {
            join.type_parameter(*name, &[])?;
        }
        for name in names {
            join.parameter(future(Type::parameter(*name)?)?);
            outputs.push(Type::parameter(*name)?);
        }
        join.output(future(Type::tuple(outputs)?)?);
        builder.item(item::Definition::function(
            format!("future::__join{count}"),
            join.finish(),
        )?)?;
    }
    builder.finish()
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn take_future(value: &mut Dynamic) -> Result<super::Future, Box<EvalAltResult>> {
    let actual = value.type_name();
    value
        .take()
        .try_cast_result::<super::Future>()
        .map_err(|_| error(format!("expected a future, found `{actual}`")))
}

/// The facade with its runtime bindings.
pub fn runtime() -> Result<host::runtime::Facade, host::Error> {
    let mut builder = host::runtime::Builder::new(definition()?);
    builder
        .bind_type_attestation(FUTURE, |value, expected| {
            expected.attest(value.is::<super::Future>())
        })
        .expect("the rawr bindings declare their future once");
    builder
        .bind_type_attestation(POLL, |value, expected| {
            expected.attest(crate::option::is(value))
        })
        .expect("the rawr bindings declare `Poll` once");
    builder
        .bind_with_context(SCRIPT, |_call, args| {
            let [step] = args else {
                return Err(error("`rawr::future::script` takes one step function"));
            };
            let actual = step.type_name();
            let step = step
                .take()
                .try_cast_result::<crate::item::Value>()
                .map_err(|_| error(format!("an async body's step is `{actual}`")))?;
            Ok(Dynamic::from(super::Future::script(step)))
        })
        .expect("the rawr bindings declare `future::script` once");
    builder
        .bind_with_context(AWAITED_PATH, |call, args| {
            let [value] = args else {
                return Err(error("`rawr::future::__awaited` takes one value"));
            };
            let actual = value.type_name();
            let not_future = || error(format!("expected a future, found `{actual}`"));
            #[cfg(feature = "rust-import")]
            let made = crate::island::awaited(value.take()).map_err(|_| not_future())?;
            #[cfg(not(feature = "rust-import"))]
            let made = value.take();
            // The future takes the witness of this call's output, whose
            // argument its output is checked as.
            let made = match made.try_cast_result::<host::runtime::Unwitnessed>() {
                Ok(pending) => {
                    let witness = call
                        .output_witness()
                        .map_err(|source| error(source.to_string()))?;
                    pending.witness(witness).ok_or_else(not_future)?
                }
                Err(made) => made,
            };
            if made.is::<super::Future>() {
                Ok(made)
            } else {
                Err(not_future())
            }
        })
        .expect("the rawr bindings declare `future::__awaited` once");
    builder
        .bind_with_context(POLL_FN, |_call, args| {
            let [poll] = args else {
                return Err(error("`poll_fn` takes one closure"));
            };
            let actual = poll.type_name();
            let poll = poll
                .take()
                .try_cast_result::<crate::item::Value>()
                .map_err(|_| error(format!("`poll_fn` takes a closure, not `{actual}`")))?;
            Ok(Dynamic::from(super::Future::poll_fn(poll)))
        })
        .expect("the rawr bindings declare `future::poll_fn` once");
    builder
        .bind_with_context(POLL_ONCE, |_call, args| {
            let [future, context] = args else {
                return Err(error("`Future::poll` takes a future and a context"));
            };
            let waker = context
                .read_lock::<super::Waking>()
                .map(|context| context.waker().clone())
                .ok_or_else(|| error("`Future::poll` takes a `Context`"))?;
            let actual = future.type_name();
            let future = future
                .read_lock::<super::Future>()
                .map(|future| future.clone())
                .ok_or_else(|| error(format!("expected a future, found `{actual}`")))?;
            Ok(Dynamic::from(future.poll_once(&waker)?))
        })
        .expect("the rawr bindings declare `future::__poll` once");
    builder
        .bind_with_context(format!("{CONTEXT}::waker"), |_call, args| {
            let [context] = args else {
                return Err(error("`Context::waker` takes its receiver"));
            };
            context
                .read_lock::<super::Waking>()
                .map(|context| Dynamic::from(context.waker().clone()))
                .ok_or_else(|| error("`Context::waker` takes a `Context`"))
        })
        .expect("the rawr bindings declare `Context::waker` once");
    for wake in ["wake", "wake_by_ref"] {
        builder
            .bind_with_context(format!("{WAKER}::{wake}"), |_call, args| {
                let [waker] = args else {
                    return Err(error("a `Waker` method takes its receiver"));
                };
                waker
                    .read_lock::<std::task::Waker>()
                    .map(|waker| waker.wake_by_ref())
                    .ok_or_else(|| error("a `Waker` is Rust's at runtime"))?;
                Ok(Dynamic::UNIT)
            })
            .expect("the rawr bindings declare each `Waker` method once");
    }
    for state in ["is_ready", "is_pending"] {
        let ready = state == "is_ready";
        builder
            .bind_with_context(format!("{POLL}::{state}"), move |_call, args| {
                let [poll] = args else {
                    return Err(error("a `Poll` method takes its receiver"));
                };
                let polled = crate::option::read(poll)
                    .ok_or_else(|| error("a `Poll` is an `Option` at runtime"))?;
                Ok(Dynamic::from(polled.is_some() == ready))
            })
            .expect("the rawr bindings declare each `Poll` method once");
    }
    for count in 2..=MOST_FUTURES {
        builder
            .bind_with_context(join_path(count), |_call, args| {
                let parts = args
                    .iter_mut()
                    .map(|part| take_future(part))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Dynamic::from(super::Future::join(parts)))
            })
            .expect("the rawr bindings declare each `future::__join` once");
    }
    Ok(builder.finish())
}
