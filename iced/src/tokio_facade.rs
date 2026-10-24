//! tokio's timers and channels for Rawr code in a program without a tokio
//! dependency, as `?play` snippets are (`resolve::std_reexport` sends their
//! `tokio::` paths here). Only `time`, `sync` and `task` items exist, so a
//! snippet cannot name tokio's files, sockets or processes.
//!
//! A future runs inside a tokio runtime. While a recording's clock is
//! frozen, that is the clock's paused runtime, which the recording moves a
//! frame at a time (`snapshot::clock::tokio`). Elsewhere it is one runtime
//! on a thread of its own, on the real clock.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use iced::futures::future::BoxFuture;
use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{receiver, Dynamic, EvalAltResult, Position};
use tokio::sync::{mpsc, oneshot, watch};

use crate::snapshot::clock;
use crate::task;

/// The runtime a tokio future made now runs in.
fn handle() -> tokio::runtime::Handle {
    clock::tokio().unwrap_or_else(|| background().clone())
}

/// One runtime on the real clock, on a thread that drives its timers.
fn background() -> &'static tokio::runtime::Handle {
    static BACKGROUND: OnceLock<tokio::runtime::Handle> = OnceLock::new();
    BACKGROUND.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a tokio runtime on the current thread builds");
        let handle = runtime.handle().clone();
        let _ = std::thread::Builder::new()
            .name("rawr tokio".into())
            .spawn(move || runtime.block_on(std::future::pending::<()>()));
        handle
    })
}

/// A future made and polled inside `handle`'s runtime.
struct Entered<F> {
    handle: tokio::runtime::Handle,
    future: Pin<Box<F>>,
}

impl<F: Future> Future for Entered<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<F::Output> {
        let _entered = self.handle.enter();
        self.future.as_mut().poll(context)
    }
}

/// tokio's `time::sleep`.
pub(crate) fn sleep(duration: std::time::Duration) -> BoxFuture<'static, ()> {
    timer_in(handle(), move || tokio::time::sleep(duration))
}

/// A timer's future, made and polled inside `handle`'s runtime. On a
/// recording's clock it keeps the clock running until it ends.
fn timer_in<F>(
    handle: tokio::runtime::Handle,
    make: impl FnOnce() -> F,
) -> BoxFuture<'static, F::Output>
where
    F: Future + Send + 'static,
{
    let armed = clock::now().is_some().then(clock::Armed::new);
    let future = {
        let _entered = handle.enter();
        Box::pin(make())
    };
    let future = Entered { handle, future };
    Box::pin(async move {
        let output = future.await;
        drop(armed);
        output
    })
}

/// tokio's `time::Interval`, in the runtime it was made in.
#[derive(Clone)]
pub(crate) struct Interval {
    handle: tokio::runtime::Handle,
    interval: Arc<tokio::sync::Mutex<tokio::time::Interval>>,
}

/// tokio's `time::error::Elapsed`.
#[derive(Clone, Debug)]
pub(crate) struct Elapsed(());

/// A sender Rawr code holds. Its copies are one sender, as a moved sender
/// is in Rust: `drop` ends it for all of them, and `clone` makes another.
/// A channel closes once each of its senders is dropped.
type Held<T> = Arc<std::sync::Mutex<Option<T>>>;

fn held<T>(value: T) -> Held<T> {
    Arc::new(std::sync::Mutex::new(Some(value)))
}

fn lock<T>(held: &Held<T>) -> std::sync::MutexGuard<'_, Option<T>> {
    held.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// tokio's `sync::mpsc::Sender`.
#[derive(Clone)]
pub(crate) struct Sender(Held<mpsc::Sender<Dynamic>>);

/// tokio's `sync::mpsc::UnboundedSender`.
#[derive(Clone)]
pub(crate) struct UnboundedSender(Held<mpsc::UnboundedSender<Dynamic>>);

/// tokio's `sync::mpsc::Receiver`. Its copies are one receiver, as a moved
/// receiver is in Rust.
#[derive(Clone)]
pub(crate) struct Receiver(Arc<tokio::sync::Mutex<mpsc::Receiver<Dynamic>>>);

/// tokio's `sync::mpsc::UnboundedReceiver`.
#[derive(Clone)]
pub(crate) struct UnboundedReceiver(Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Dynamic>>>);

/// tokio's `sync::mpsc::error::SendError`.
#[derive(Clone)]
pub(crate) struct SendError(());

/// tokio's `sync::oneshot::Sender`, which sends once.
#[derive(Clone)]
pub(crate) struct OneshotSender(Held<oneshot::Sender<Dynamic>>);

/// tokio's `sync::oneshot::error::RecvError`.
#[derive(Clone, Debug)]
pub(crate) struct RecvError(());

/// tokio's `sync::watch::Sender`.
#[derive(Clone)]
pub(crate) struct WatchSender(Held<watch::Sender<Dynamic>>);

/// tokio's `sync::watch::Receiver`. Its copies are one receiver, which has
/// seen what any of them has, and `clone` makes another.
#[derive(Clone)]
pub(crate) struct WatchReceiver(Arc<tokio::sync::Mutex<watch::Receiver<Dynamic>>>);

/// tokio's `sync::watch::error::SendError`.
#[derive(Clone)]
pub(crate) struct WatchSendError(());

/// tokio's `sync::watch::error::RecvError`.
#[derive(Clone, Debug)]
pub(crate) struct WatchRecvError(());

macro_rules! opaque_debug {
    ($($type:ty => $text:literal),* $(,)?) => {
        $(impl fmt::Debug for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str($text)
            }
        })*
    };
}

opaque_debug! {
    Interval => "Interval { .. }",
    Sender => "Sender { .. }",
    UnboundedSender => "UnboundedSender { .. }",
    Receiver => "Receiver { .. }",
    UnboundedReceiver => "UnboundedReceiver { .. }",
    SendError => "SendError { .. }",
    OneshotSender => "Sender { .. }",
    WatchSender => "Sender { .. }",
    WatchReceiver => "Receiver { .. }",
    WatchSendError => "SendError { .. }",
}

/// Register `Debug` for each tokio type.
pub(crate) fn register_debug() {
    use rawr::host::debug::register;
    register::<Interval>();
    register::<Elapsed>();
    register::<Sender>();
    register::<UnboundedSender>();
    register::<Receiver>();
    register::<UnboundedReceiver>();
    register::<SendError>();
    register::<OneshotSender>();
    register::<RecvError>();
    register::<WatchSender>();
    register::<WatchReceiver>();
    register::<WatchSendError>();
    register::<WatchRecvError>();
}

/// The most futures `join!`, `try_join!` and `select!` take.
pub(crate) const MOST_FUTURES: usize = 5;

/// Declare tokio's items in rawr-iced's facade, under `tokio`.
pub(crate) fn declare(
    builder: &mut facade::Builder,
    task: &host::type_def::Export,
    duration: &host::type_def::Export,
    instant: &host::type_def::Export,
) -> Result<(), host::Error> {
    use rawr::capability::Kind::{Clone, Debug};
    use type_ref::Type;

    for module in [
        "tokio",
        "tokio::task",
        "tokio::time",
        "tokio::time::error",
        "tokio::sync",
        "tokio::sync::mpsc",
        "tokio::sync::mpsc::error",
        "tokio::sync::oneshot",
        "tokio::sync::oneshot::error",
        "tokio::sync::watch",
        "tokio::sync::watch::error",
    ] {
        builder.module(module)?;
    }
    builder.type_::<Interval>("tokio::time::Interval", &[], &[Debug])?;
    builder.type_::<Elapsed>("tokio::time::error::Elapsed", &[], &[Debug, Clone])?;
    builder.type_::<Sender>("tokio::sync::mpsc::Sender", &["T"], &[Debug, Clone])?;
    builder.type_::<Receiver>("tokio::sync::mpsc::Receiver", &["T"], &[Debug])?;
    builder.type_::<UnboundedSender>(
        "tokio::sync::mpsc::UnboundedSender",
        &["T"],
        &[Debug, Clone],
    )?;
    builder.type_::<UnboundedReceiver>("tokio::sync::mpsc::UnboundedReceiver", &["T"], &[Debug])?;
    builder.type_::<SendError>("tokio::sync::mpsc::error::SendError", &["T"], &[Debug])?;
    builder.type_::<OneshotSender>("tokio::sync::oneshot::Sender", &["T"], &[Debug])?;
    builder.type_::<RecvError>(
        "tokio::sync::oneshot::error::RecvError",
        &[],
        &[Debug, Clone],
    )?;
    // A mutex is a Rawr `RefCell` at runtime, which `mutex.lock().await`
    // borrows once no other borrow holds it
    // (`resolve::lower_nominal::cell::parse_mutex_lock`).
    builder.type_::<rawr::resolve::Cell>("tokio::sync::Mutex", &["T"], &[])?;
    builder.type_::<WatchSender>("tokio::sync::watch::Sender", &["T"], &[Debug])?;
    builder.type_::<WatchReceiver>("tokio::sync::watch::Receiver", &["T"], &[Debug, Clone])?;
    builder.type_::<WatchSendError>("tokio::sync::watch::error::SendError", &["T"], &[Debug])?;
    builder.type_::<WatchRecvError>(
        "tokio::sync::watch::error::RecvError",
        &[],
        &[Debug, Clone],
    )?;

    let parameter = |name: &str| Type::parameter(name);
    let task_of = |output: Type| type_ref::Type::external(task.clone(), [output]);
    let duration = || type_ref::Type::external(duration.clone(), []);
    let generic = |path: &str| Type::applied(path, [parameter("T")?]);
    let unit = Type::unit;

    let function = |builder: &mut facade::Builder,
                    path: &str,
                    parameters: &[&str],
                    inputs: Vec<Type>,
                    output: Type|
     -> Result<(), host::Error> {
        let mut signature = signature::Builder::new();
        for name in parameters {
            signature.type_parameter(*name, &[])?;
        }
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        builder.item(item::Definition::function(path, signature.finish())?)?;
        Ok(())
    };
    let method = |builder: &mut facade::Builder,
                  path: &str,
                  mode: receiver::Mode,
                  generic: bool,
                  inputs: Vec<Type>,
                  output: Type|
     -> Result<(), host::Error> {
        let mut signature = signature::Builder::new();
        if generic {
            signature.type_parameter("T", &[])?;
        }
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        builder.item(item::Definition::method(path, mode, signature.finish())?)?;
        Ok(())
    };
    use receiver::Mode::{Owned, Shared};

    // time
    function(
        builder,
        "tokio::time::sleep",
        &[],
        vec![duration()?],
        task_of(unit())?,
    )?;
    function(
        builder,
        "tokio::time::interval",
        &[],
        vec![duration()?],
        Type::registered("tokio::time::Interval")?,
    )?;
    method(
        builder,
        "tokio::time::Interval::tick",
        Shared,
        false,
        vec![],
        task_of(type_ref::Type::external(instant.clone(), [])?)?,
    )?;
    function(
        builder,
        "tokio::time::timeout",
        &["T"],
        vec![duration()?, task_of(parameter("T")?)?],
        task_of(Type::result(
            parameter("T")?,
            Type::registered("tokio::time::error::Elapsed")?,
        ))?,
    )?;
    // task
    function(
        builder,
        "tokio::task::yield_now",
        &[],
        vec![],
        task_of(unit())?,
    )?;

    // mpsc
    let usize = Type::builtin(type_ref::Builtin::Usize);
    for (prefix, sender, receiver, capacity) in [
        ("tokio::sync::mpsc::channel", "Sender", "Receiver", true),
        (
            "tokio::sync::mpsc::unbounded_channel",
            "UnboundedSender",
            "UnboundedReceiver",
            false,
        ),
    ] {
        let sender = format!("tokio::sync::mpsc::{sender}");
        let receiver = format!("tokio::sync::mpsc::{receiver}");
        function(
            builder,
            prefix,
            &["T"],
            if capacity {
                vec![usize.clone()]
            } else {
                vec![]
            },
            Type::tuple([generic(&sender)?, generic(&receiver)?])?,
        )?;
        let send_error = Type::result(unit(), generic("tokio::sync::mpsc::error::SendError")?);
        method(
            builder,
            &format!("{sender}::send"),
            Shared,
            true,
            vec![parameter("T")?],
            if capacity {
                task_of(send_error)?
            } else {
                send_error
            },
        )?;
        method(
            builder,
            &format!("{receiver}::recv"),
            Shared,
            true,
            vec![],
            task_of(Type::option(parameter("T")?))?,
        )?;
    }

    // oneshot: its receiver is the task of the value it receives.
    function(
        builder,
        "tokio::sync::oneshot::channel",
        &["T"],
        vec![],
        Type::tuple([
            generic("tokio::sync::oneshot::Sender")?,
            task_of(Type::result(
                parameter("T")?,
                Type::registered("tokio::sync::oneshot::error::RecvError")?,
            ))?,
        ])?,
    )?;
    method(
        builder,
        "tokio::sync::oneshot::Sender::send",
        Owned,
        true,
        vec![parameter("T")?],
        Type::result(unit(), parameter("T")?),
    )?;

    // Mutex
    let mut new = signature::Builder::new();
    new.type_parameter("T", &[])?;
    new.parameter(parameter("T")?);
    new.output(generic("tokio::sync::Mutex")?);
    builder.item(item::Definition::associated_function("tokio::sync::Mutex::new", new.finish())?)?;
    function(
        builder,
        "tokio::__lock",
        &["T"],
        vec![generic("tokio::sync::Mutex")?],
        task_of(unit())?,
    )?;

    // watch
    function(
        builder,
        "tokio::sync::watch::channel",
        &["T"],
        vec![parameter("T")?],
        Type::tuple([
            generic("tokio::sync::watch::Sender")?,
            generic("tokio::sync::watch::Receiver")?,
        ])?,
    )?;
    method(
        builder,
        "tokio::sync::watch::Sender::send",
        Shared,
        true,
        vec![parameter("T")?],
        Type::result(unit(), generic("tokio::sync::watch::error::SendError")?),
    )?;
    method(
        builder,
        "tokio::sync::watch::Sender::subscribe",
        Shared,
        true,
        vec![],
        generic("tokio::sync::watch::Receiver")?,
    )?;
    method(
        builder,
        "tokio::sync::watch::Receiver::changed",
        Shared,
        true,
        vec![],
        task_of(Type::result(
            unit(),
            Type::registered("tokio::sync::watch::error::RecvError")?,
        ))?,
    )?;
    for name in ["borrow", "borrow_and_update"] {
        method(
            builder,
            &format!("tokio::sync::watch::Receiver::{name}"),
            Shared,
            true,
            vec![],
            parameter("T")?,
        )?;
    }

    // `sender.clone()`, another sender, and `drop(sender)`, which ends one.
    for (path, generic_type) in [
        ("tokio::sync::mpsc::Sender", true),
        ("tokio::sync::mpsc::UnboundedSender", true),
        ("tokio::sync::watch::Receiver", true),
    ] {
        method(
            builder,
            &format!("{path}::clone"),
            Shared,
            generic_type,
            vec![],
            generic(path)?,
        )?;
    }
    function(
        builder,
        "tokio::__drop",
        &["T"],
        vec![parameter("T")?],
        unit(),
    )?;

    // `join!`, `try_join!` and `select!`, which the compiler writes as
    // calls of these for each count of futures.
    const NAMES: [&str; MOST_FUTURES] = ["A", "B", "C", "D", "F"];
    for count in 2..=MOST_FUTURES {
        let names = &NAMES[..count];
        let mut join = Vec::new();
        let mut try_join = Vec::new();
        for name in names {
            join.push(task_of(parameter(*name)?)?);
            try_join.push(task_of(Type::result(parameter(*name)?, parameter("E")?))?);
        }
        let outputs = names
            .iter()
            .map(|name| parameter(*name))
            .collect::<Result<Vec<_>, _>>()?;
        function(
            builder,
            &format!("tokio::__join{count}"),
            names,
            join.clone(),
            task_of(Type::tuple(outputs.clone())?)?,
        )?;
        let mut fallible = names.to_vec();
        fallible.push("E");
        function(
            builder,
            &format!("tokio::__try_join{count}"),
            &fallible,
            try_join,
            task_of(Type::result(Type::tuple(outputs.clone())?, parameter("E")?))?,
        )?;
        let mut selected = outputs[count - 1].clone();
        for output in outputs[..count - 1].iter().rev() {
            selected = Type::result(output.clone(), selected);
        }
        function(
            builder,
            &format!("tokio::__select{count}"),
            names,
            join,
            task_of(selected)?,
        )?;
    }
    Ok(())
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn read<T: std::clone::Clone + 'static>(
    value: &Dynamic,
    expected: &str,
) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            error(format!(
                "expected `{expected}`, found `{}`",
                value.type_name()
            ))
        })
}

/// The output witness of a function's call.
fn witness(call: &host::runtime::Call<'_>) -> Result<host::runtime::Witness, Box<EvalAltResult>> {
    call.output_witness().map_err(|witness| error(witness.to_string()))
}

fn ready(value: Dynamic) -> BoxFuture<'static, Dynamic> {
    Box::pin(std::future::ready(value))
}

/// Bind tokio's items in rawr-iced's facade.
pub(crate) fn bind(rawr: &mut host::runtime::Builder) {
    macro_rules! attest {
        ($($path:literal => $type:ty),* $(,)?) => {
            $(rawr.bind_type_attestation(concat!("rawr_iced::", $path), |value, expected| {
                expected.attest(value.read_lock::<$type>().is_some())
            })
            .expect(concat!("the rawr_iced facade must declare ", $path, " exactly once"));)*
        };
    }
    rawr.bind_type_attestation("rawr_iced::tokio::sync::Mutex", |value, expected| {
        expected.attest(rawr::resolve::cell_held(value).is_some())
    })
    .expect("the rawr_iced facade must declare tokio::sync::Mutex exactly once");
    attest! {
        "tokio::sync::mpsc::Sender" => Sender,
        "tokio::sync::mpsc::Receiver" => Receiver,
        "tokio::sync::mpsc::UnboundedSender" => UnboundedSender,
        "tokio::sync::mpsc::UnboundedReceiver" => UnboundedReceiver,
        "tokio::sync::mpsc::error::SendError" => SendError,
        "tokio::sync::oneshot::Sender" => OneshotSender,
        "tokio::sync::watch::Sender" => WatchSender,
        "tokio::sync::watch::Receiver" => WatchReceiver,
        "tokio::sync::watch::error::SendError" => WatchSendError,
    }
    let mut bind = |path: &str,
                    f: fn(
        &host::runtime::Call<'_>,
        &mut [&mut Dynamic],
    ) -> Result<Dynamic, Box<EvalAltResult>>| {
        rawr.bind_with_context(&format!("rawr_iced::{path}"), move |call, args| {
            f(call, args)
        })
        .unwrap_or_else(|_| panic!("the rawr_iced facade must retain {path}"));
    };
    bind("tokio::time::sleep", |call, args| {
        let duration = read::<std::time::Duration>(args[0], "Duration")?;
        let witness = witness(call)?;
        Ok(task::tokio_task(Some(witness), move || {
            let sleep = sleep(duration);
            Box::pin(async move {
                sleep.await;
                Dynamic::UNIT
            })
        }))
    });
    bind("tokio::time::interval", |_call, args| {
        let period = read::<std::time::Duration>(args[0], "Duration")?;
        if period.is_zero() {
            return Err(error("`period` must be non-zero."));
        }
        let handle = handle();
        let interval = {
            let _entered = handle.enter();
            tokio::time::interval(period)
        };
        Ok(Dynamic::from(Interval {
            handle,
            interval: Arc::new(tokio::sync::Mutex::new(interval)),
        }))
    });
    bind("tokio::time::Interval::tick", |_call, args| {
        let interval = read::<Interval>(args[0], "Interval")?;
        Ok(task::tokio_task(None, move || {
            let shared = interval.interval.clone();
            let tick = timer_in(interval.handle.clone(), move || async move {
                shared.lock().await.tick().await;
            });
            Box::pin(async move {
                tick.await;
                Dynamic::from(crate::marker::Instant::now())
            })
        }))
    });
    bind("tokio::time::timeout", |call, args| {
        let [duration, plan] = args else {
            return Err(error("`timeout` takes a duration and a future"));
        };
        let duration = read::<std::time::Duration>(duration, "Duration")?;
        let witness = witness(call)?;
        task::tokio_timeout(witness, plan, duration, Dynamic::from(Elapsed(())))
    });
    bind("tokio::task::yield_now", |call, _args| {
        // Each step of Rawr code already goes back to the UI thread, so the
        // other tasks run between this one's steps.
        let witness = witness(call)?;
        Ok(task::tokio_task(Some(witness), || ready(Dynamic::UNIT)))
    });

    bind("tokio::sync::mpsc::channel", |_call, args| {
        let capacity = args[0]
            .as_int()
            .map_err(|_| error("a channel's capacity is a `usize`"))?;
        let capacity = usize::try_from(capacity)
            .ok()
            .filter(|&capacity| capacity > 0);
        let Some(capacity) = capacity else {
            return Err(error("mpsc bounded channel requires buffer > 0"));
        };
        let (sender, receiver) = mpsc::channel(capacity);
        Ok(Dynamic::from_array(vec![
            Dynamic::from(Sender(held(sender))),
            Dynamic::from(Receiver(Arc::new(tokio::sync::Mutex::new(receiver)))),
        ]))
    });
    bind("tokio::sync::mpsc::unbounded_channel", |_call, _args| {
        let (sender, receiver) = mpsc::unbounded_channel();
        Ok(Dynamic::from_array(vec![
            Dynamic::from(UnboundedSender(held(sender))),
            Dynamic::from(UnboundedReceiver(Arc::new(tokio::sync::Mutex::new(
                receiver,
            )))),
        ]))
    });
    bind("tokio::sync::mpsc::Sender::send", |_call, args| {
        let sender = read::<Sender>(args[0], "mpsc::Sender")?;
        let value = args[1].take();
        Ok(task::tokio_task(None, move || {
            let sender = lock(&sender.0).clone();
            let value = value.clone();
            Box::pin(async move {
                let Some(sender) = sender else {
                    return rawr::result::err(Dynamic::from(SendError(())));
                };
                match sender.send(value).await {
                    Ok(()) => rawr::result::ok(Dynamic::UNIT),
                    Err(_) => rawr::result::err(Dynamic::from(SendError(()))),
                }
            })
        }))
    });
    bind("tokio::sync::mpsc::UnboundedSender::send", |_call, args| {
        let sender = read::<UnboundedSender>(args[0], "mpsc::UnboundedSender")?;
        let sent = lock(&sender.0)
            .as_ref()
            .map(|sender| sender.send(args[1].take()));
        Ok(
            match sent.unwrap_or(Err(mpsc::error::SendError(Dynamic::UNIT))) {
                Ok(()) => rawr::result::ok(Dynamic::UNIT),
                Err(_) => rawr::result::err(Dynamic::from(SendError(()))),
            },
        )
    });
    bind("tokio::sync::mpsc::Receiver::recv", |_call, args| {
        let receiver = read::<Receiver>(args[0], "mpsc::Receiver")?;
        Ok(task::tokio_task(None, move || {
            let receiver = receiver.0.clone();
            Box::pin(async move {
                let received = receiver.lock().await.recv().await;
                received.map_or_else(rawr::option::none, rawr::option::some)
            })
        }))
    });
    bind(
        "tokio::sync::mpsc::UnboundedReceiver::recv",
        |_call, args| {
            let receiver = read::<UnboundedReceiver>(args[0], "mpsc::UnboundedReceiver")?;
            Ok(task::tokio_task(None, move || {
                let receiver = receiver.0.clone();
                Box::pin(async move {
                    let received = receiver.lock().await.recv().await;
                    received.map_or_else(rawr::option::none, rawr::option::some)
                })
            }))
        },
    );

    bind("tokio::sync::oneshot::channel", |_call, _args| {
        let (sender, receiver) = oneshot::channel::<Dynamic>();
        let receiver = Arc::new(std::sync::Mutex::new(Some(receiver)));
        let received = task::tokio_task(None, move || {
            let receiver = receiver
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            Box::pin(async move {
                match receiver {
                    Some(receiver) => match receiver.await {
                        Ok(value) => rawr::result::ok(value),
                        Err(_) => rawr::result::err(Dynamic::from(RecvError(()))),
                    },
                    None => rawr::result::err(Dynamic::from(RecvError(()))),
                }
            })
        });
        Ok(Dynamic::from_array(vec![
            Dynamic::from(OneshotSender(held(sender))),
            received,
        ]))
    });
    bind("tokio::sync::oneshot::Sender::send", |_call, args| {
        let sender = read::<OneshotSender>(args[0], "oneshot::Sender")?;
        let value = args[1].take();
        let sender = lock(&sender.0).take();
        Ok(match sender {
            Some(sender) => match sender.send(value) {
                Ok(()) => rawr::result::ok(Dynamic::UNIT),
                Err(value) => rawr::result::err(value),
            },
            None => rawr::result::err(value),
        })
    });

    bind("tokio::sync::Mutex::new", |_call, args| Ok(rawr::resolve::cell_new(args[0].take())));
    bind("tokio::__lock", |_call, args| {
        // A borrow's end wakes nothing, so a waiting task looks again. On a
        // recording's clock it asks to be polled again at once, and the
        // recording polls it after each step of Rawr code. Elsewhere it looks
        // after each millisecond. The borrow is made as the task ends, so no
        // other task takes it between this one's end and its next step.
        let mutex = args[0].clone();
        Ok(task::tokio_task(None, move || {
            let mutex = mutex.clone();
            let recording = clock::now().is_some();
            let mut timer: Option<BoxFuture<'static, ()>> = None;
            Box::pin(std::future::poll_fn(move |context| loop {
                if let Some(guard) = rawr::resolve::cell_lock(&mutex) {
                    return Poll::Ready(guard);
                }
                if recording {
                    context.waker().wake_by_ref();
                    return Poll::Pending;
                }
                let waiting = timer.get_or_insert_with(|| sleep(std::time::Duration::from_millis(1)));
                if waiting.as_mut().poll(context).is_pending() {
                    return Poll::Pending;
                }
                timer = None;
            }))
        }))
    });
    bind("tokio::sync::watch::channel", |_call, args| {
        let (sender, receiver) = watch::channel(args[0].take());
        Ok(Dynamic::from_array(vec![
            Dynamic::from(WatchSender(held(sender))),
            Dynamic::from(WatchReceiver(Arc::new(tokio::sync::Mutex::new(receiver)))),
        ]))
    });
    bind("tokio::sync::watch::Sender::send", |_call, args| {
        let sender = read::<WatchSender>(args[0], "watch::Sender")?;
        let sent = lock(&sender.0)
            .as_ref()
            .map(|sender| sender.send(args[1].take()));
        Ok(
            match sent.unwrap_or(Err(watch::error::SendError(Dynamic::UNIT))) {
                Ok(()) => rawr::result::ok(Dynamic::UNIT),
                Err(_) => rawr::result::err(Dynamic::from(WatchSendError(()))),
            },
        )
    });
    bind("tokio::sync::watch::Sender::subscribe", |_call, args| {
        let sender = read::<WatchSender>(args[0], "watch::Sender")?;
        let receiver = lock(&sender.0)
            .as_ref()
            .map(watch::Sender::subscribe)
            .ok_or_else(|| error("the sender was dropped"))?;
        Ok(Dynamic::from(WatchReceiver(Arc::new(
            tokio::sync::Mutex::new(receiver),
        ))))
    });
    bind("tokio::sync::watch::Receiver::changed", |_call, args| {
        let receiver = read::<WatchReceiver>(args[0], "watch::Receiver")?;
        Ok(task::tokio_task(None, move || {
            let receiver = receiver.0.clone();
            Box::pin(async move {
                match receiver.lock().await.changed().await {
                    Ok(()) => rawr::result::ok(Dynamic::UNIT),
                    Err(_) => rawr::result::err(Dynamic::from(WatchRecvError(()))),
                }
            })
        }))
    });
    bind("tokio::sync::watch::Receiver::borrow", |_call, args| {
        let receiver = read::<WatchReceiver>(args[0], "watch::Receiver")?;
        let receiver = receiver
            .0
            .try_lock()
            .map_err(|_| error("the receiver is waiting for a change"))?;
        Ok(receiver.borrow().clone())
    });
    bind(
        "tokio::sync::watch::Receiver::borrow_and_update",
        |_call, args| {
            let receiver = read::<WatchReceiver>(args[0], "watch::Receiver")?;
            let mut receiver = receiver
                .0
                .try_lock()
                .map_err(|_| error("the receiver is waiting for a change"))?;
            Ok(receiver.borrow_and_update().clone())
        },
    );

    bind("tokio::sync::mpsc::Sender::clone", |_call, args| {
        let sender = read::<Sender>(args[0], "mpsc::Sender")?;
        let cloned = lock(&sender.0).clone();
        Ok(Dynamic::from(Sender(Arc::new(std::sync::Mutex::new(
            cloned,
        )))))
    });
    bind(
        "tokio::sync::mpsc::UnboundedSender::clone",
        |_call, args| {
            let sender = read::<UnboundedSender>(args[0], "mpsc::UnboundedSender")?;
            let cloned = lock(&sender.0).clone();
            Ok(Dynamic::from(UnboundedSender(Arc::new(
                std::sync::Mutex::new(cloned),
            ))))
        },
    );
    bind("tokio::sync::watch::Receiver::clone", |_call, args| {
        let receiver = read::<WatchReceiver>(args[0], "watch::Receiver")?;
        let cloned = receiver
            .0
            .try_lock()
            .map_err(|_| error("the receiver is waiting for a change"))?
            .clone();
        Ok(Dynamic::from(WatchReceiver(Arc::new(
            tokio::sync::Mutex::new(cloned),
        ))))
    });
    bind("tokio::__drop", |_call, args| {
        let value = &*args[0];
        if let Some(sender) = value.read_lock::<Sender>() {
            lock(&sender.0).take();
        } else if let Some(sender) = value.read_lock::<UnboundedSender>() {
            lock(&sender.0).take();
        } else if let Some(sender) = value.read_lock::<WatchSender>() {
            lock(&sender.0).take();
        } else if let Some(sender) = value.read_lock::<OneshotSender>() {
            lock(&sender.0).take();
        }
        Ok(Dynamic::UNIT)
    });

    for count in 2..=MOST_FUTURES {
        for (name, join) in [
            ("__join", Some(false)),
            ("__try_join", Some(true)),
            ("__select", None),
        ] {
            let path = format!("rawr_iced::tokio::{name}{count}");
            let what = path.clone();
            rawr.bind_with_context(&path, move |call, args| {
                let witness = witness(call)?;
                task::tokio_combined(witness, args, join, &what)
            })
            .unwrap_or_else(|_| panic!("the rawr_iced facade must retain {path}"));
        }
    }
}
