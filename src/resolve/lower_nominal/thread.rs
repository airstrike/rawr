//! `std::thread::spawn` and its `JoinHandle<T>`, `thread::scope` and its
//! `Scope` and `ScopedJoinHandle<T>`, and `std::sync::mpsc`'s `channel`,
//! `sync_channel`, `Sender<T>`, `SyncSender<T>` and `Receiver<T>`. Each
//! spawned closure runs on a thread of its own, in parallel with the others
//! (`eval::threads`), and a channel is std's own, carrying rawr's values.
//! Which methods each type has is std's (`std_method`), and so are the
//! errors a receive gives (`std_facade::sync`); how rawr runs each is here.
//!
//! A closure `spawn` takes must be `Send`, as std's bound says: one that
//! captures what rawr knows isn't `Send`, as an `Rc`, is rustc's E0277. A
//! scoped thread may borrow what outlives its scope, which joins it.

use super::*;

/// Which of std's thread and channel types a `Ty::Thread` is, with the
/// errors their methods give.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Kind {
    JoinHandle,
    Sender,
    /// A `sync_channel`'s sending half, whose `send` waits while the
    /// channel holds as many values as it is bounded to.
    SyncSender,
    Receiver,
    /// `SendError<T>`, of a send no receiver takes.
    SendError,
    /// `Box<dyn Any + Send>`, the panic `join` gives; its `Ty::Thread`
    /// holds `()`.
    Panic,
    /// `thread::Scope`, whose threads `scope` joins before it returns; its
    /// `Ty::Thread` holds `()`.
    Scope,
    /// A scoped thread's `ScopedJoinHandle<T>`.
    ScopedJoinHandle,
}

impl Kind {
    /// The kind rawr's builtin type `name` is, as `type_ref` names them.
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "JoinHandle" => Self::JoinHandle,
            "ScopedJoinHandle" => Self::ScopedJoinHandle,
            crate::resolve::MPSC_SENDER => Self::Sender,
            crate::resolve::MPSC_SYNC_SENDER => Self::SyncSender,
            crate::resolve::MPSC_RECEIVER => Self::Receiver,
            _ => return None,
        })
    }

    /// The type's name, as rustc writes it with `inner`.
    pub(super) fn name(self, inner: &str) -> String {
        match self {
            Self::JoinHandle => format!("JoinHandle<{inner}>"),
            Self::Sender => format!("Sender<{inner}>"),
            Self::SyncSender => format!("SyncSender<{inner}>"),
            Self::Receiver => format!("Receiver<{inner}>"),
            Self::SendError => format!("SendError<{inner}>"),
            Self::Panic => "Box<dyn Any + Send>".into(),
            Self::Scope => "Scope<'_, '_>".into(),
            Self::ScopedJoinHandle => format!("ScopedJoinHandle<'_, {inner}>"),
        }
    }

    /// std's definition of the type, whose inherent methods and impls it
    /// has, and whether it takes the inner type as its argument. `None`
    /// for `join`'s `Box<dyn Any + Send>`, which is no std type rawr names.
    pub(super) const fn definition(self) -> Option<(&'static str, bool)> {
        Some(match self {
            Self::JoinHandle => ("std::thread::join_handle::JoinHandle", true),
            Self::Sender => ("std::sync::mpsc::Sender", true),
            Self::SyncSender => ("std::sync::mpsc::SyncSender", true),
            Self::Receiver => ("std::sync::mpsc::Receiver", true),
            Self::SendError => ("std::sync::mpsc::SendError", true),
            Self::Panic => return None,
            Self::Scope => ("std::thread::scoped::Scope", false),
            Self::ScopedJoinHandle => ("std::thread::scoped::ScopedJoinHandle", true),
        })
    }

    /// Whether a value of the kind holding `inner` is `Send` where
    /// `inner` is: every one but `Box<dyn Any + Send>`, which always is.
    pub(super) const fn sends(self) -> bool {
        !matches!(self, Self::Panic)
    }

    /// Whether the kind may be shared between threads where `inner` is
    /// `Send`: a `Receiver` may not.
    pub(super) const fn syncs(self) -> bool {
        !matches!(self, Self::Receiver | Self::Panic)
    }

    /// Whether std implements `Clone` for the kind: a sending half only.
    pub(super) const fn clones(self) -> bool {
        matches!(self, Self::Sender | Self::SyncSender)
    }
}

/// The functions and methods rawr runs, by the code `invoke` takes.
const METHODS: [&str; 16] = [
    "spawn",
    "join",
    "is_finished",
    "channel",
    "send",
    "recv",
    "try_recv",
    "clone",
    "iter",
    "debug",
    "display",
    "recv_timeout",
    "sync_channel",
    "scope",
    "scoped_spawn",
    "builder_spawn",
];

fn code(name: &str) -> origin::Fragment {
    let code = METHODS
        .iter()
        .position(|method| *method == name)
        .expect("a listed thread method");
    code.to_string().into()
}

/// What starts a thread: `thread::spawn`, a `thread::Builder`'s `spawn`,
/// whose closures must live for the program, or a `Scope`'s, whose closure
/// must live for the scope that joins it.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Spawner {
    Function,
    Builder,
    Scope,
}

impl Spawner {
    /// The std function whose bounds the closure meets, as rustc names it.
    const fn path(self) -> &'static str {
        match self {
            Self::Function => "std::thread::spawn",
            Self::Builder => "Builder::spawn",
            Self::Scope => "Scope::<'scope, 'env>::spawn",
        }
    }
}

/// Whether a channel holds at most a bound of values, as `sync_channel`'s.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Bound {
    Unbounded,
    Bounded,
}

/// A channel's sending half, std's own.
#[derive(Clone)]
pub(crate) struct Sender(std::sync::mpsc::Sender<Dynamic>);

/// A bounded channel's sending half, std's own.
#[derive(Clone)]
pub(crate) struct SyncSender(std::sync::mpsc::SyncSender<Dynamic>);

/// A `thread::scope`'s threads, which it joins before it returns.
#[cfg(feature = "sync")]
#[derive(Clone)]
pub(crate) struct Scope(crate::Shared<std::sync::Mutex<Vec<crate::eval::threads::Handle>>>);

/// A channel's receiving half, std's own, which one thread at a time
/// receives from.
#[derive(Clone)]
pub(crate) struct Receiver(crate::Shared<std::sync::Mutex<std::sync::mpsc::Receiver<Dynamic>>>);

impl Receiver {
    /// Each value the channel receives, waiting for the next, while a
    /// sender is left: what `Receiver::iter` gives, as a `for` walks it.
    pub(crate) fn values(value: Dynamic) -> Box<dyn Iterator<Item = Dynamic>> {
        let Some(receiver) = value.read_lock::<Self>().map(|receiver| receiver.0.clone()) else {
            return Box::new(std::iter::empty());
        };
        Box::new(std::iter::from_fn(move || {
            receiver
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .recv()
                .ok()
        }))
    }
}

/// The payload `join` gives of a thread that panicked, whose `Debug` is
/// `Any { .. }`: rawr's panic is its message, which the thread printed.
#[derive(Clone)]
pub(crate) struct Panic;

/// A send no receiver took. Its value is dropped with it.
#[derive(Clone)]
pub(crate) struct SendError;

/// `thread(value, code, operand)`: the function or method `METHODS` names
/// by `code`, on `value` with `operand`, with std's panics.
pub(super) fn invoke(context: Option<&NativeCallContext<'_>>, args: &mut FnCallArgs) -> RawrResult {
    let code = usize::try_from(args[1].as_int().unwrap_or(-1)).unwrap_or(usize::MAX);
    match METHODS.get(code).copied().unwrap_or("") {
        "spawn" => spawn(context, args[0].take()),
        "join" => join(args[0]),
        "is_finished" => is_finished(args[0]),
        "channel" => {
            let (sender, receiver) = std::sync::mpsc::channel();
            Ok(halves(Dynamic::from(Sender(sender)), receiver))
        }
        "sync_channel" => {
            let bound = usize::try_from(args[2].as_int().unwrap_or(0)).unwrap_or(usize::MAX);
            let (sender, receiver) = std::sync::mpsc::sync_channel(bound);
            Ok(halves(Dynamic::from(SyncSender(sender)), receiver))
        }
        "send" => {
            let value = args[2].take();
            let sent = match args[0].read_lock::<Sender>().map(|sender| sender.0.clone()) {
                Some(sender) => sender.send(value),
                None => args[0]
                    .read_lock::<SyncSender>()
                    .map(|sender| sender.0.clone())
                    .ok_or_else(|| invariant("a `Sender` is not one at runtime"))?
                    .send(value),
            };
            Ok(match sent {
                Ok(()) => crate::result::ok(Dynamic::UNIT),
                Err(_) => crate::result::err(Dynamic::from(SendError)),
            })
        }
        "scope" => scope(context, args[0].take()),
        "builder_spawn" => {
            let closure = args[2].take();
            builder_spawn(context, args[0], closure)
        }
        "scoped_spawn" => {
            let closure = args[2].take();
            scoped_spawn(context, args[0], closure)
        }
        // The receiver itself, which a `for` walks (`Receiver::values`).
        "iter" => Ok(args[0].clone()),
        // Each error is std's own (`std_facade::sync`).
        "recv" | "try_recv" | "recv_timeout" => {
            let receiver = args[0]
                .read_lock::<Receiver>()
                .map(|receiver| receiver.0.clone())
                .ok_or_else(|| invariant("a `Receiver` is not one at runtime"))?;
            let receiver = receiver
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let received = match METHODS[code] {
                "recv" => receiver.recv().map_err(Dynamic::from),
                "try_recv" => receiver.try_recv().map_err(Dynamic::from),
                _ => {
                    let timeout = args[2]
                        .read_lock::<std::time::Duration>()
                        .map(|timeout| *timeout)
                        .ok_or_else(|| invariant("a `Duration` is not Rust's at runtime"))?;
                    receiver.recv_timeout(timeout).map_err(Dynamic::from)
                }
            };
            Ok(match received {
                Ok(value) => crate::result::ok(value),
                Err(error) => crate::result::err(error),
            })
        }
        "clone" => Ok(args[0].clone()),
        "debug" => Ok(Dynamic::from(ImmutableString::from(debug(args[0])))),
        "display" => Ok(Dynamic::from(ImmutableString::from(display()))),
        _ => Err(invariant("not a `std::thread` or `mpsc` operation")),
    }
}

#[cfg(feature = "sync")]
fn spawn(context: Option<&NativeCallContext<'_>>, closure: Dynamic) -> RawrResult {
    let context = context.ok_or_else(|| invariant("a thread is spawned outside a call"))?;
    let closure = closure
        .try_cast::<crate::FnPtr>()
        .ok_or_else(|| invariant("a thread's closure is no function"))?;
    let handle = context
        .global_runtime_state()
        .threads
        .spawn(context, closure)?;
    Ok(Dynamic::from(handle))
}

/// Without `sync`, rawr's values stay on one thread.
#[cfg(not(feature = "sync"))]
fn spawn(_: Option<&NativeCallContext<'_>>, _: Dynamic) -> RawrResult {
    Err(invariant(
        "`std::thread::spawn` needs rawr built with its `sync` feature",
    ))
}

/// A channel's two halves, as `channel` and `sync_channel` give them.
fn halves(sender: Dynamic, receiver: std::sync::mpsc::Receiver<Dynamic>) -> Dynamic {
    let receiver = Receiver(crate::Shared::new(std::sync::Mutex::new(receiver)));
    Dynamic::from_array(vec![sender, Dynamic::from(receiver)])
}

/// `thread::scope(f)`: `f` called with a new scope, then each thread it
/// spawned joined, as std's `scope` does before it returns. A scoped
/// thread that panicked and that `f` didn't join is std's panic.
#[cfg(feature = "sync")]
fn scope(context: Option<&NativeCallContext<'_>>, closure: Dynamic) -> RawrResult {
    let context = context.ok_or_else(|| invariant("a scope is made outside a call"))?;
    let closure = closure
        .try_cast::<crate::FnPtr>()
        .ok_or_else(|| invariant("a scope's closure is no function"))?;
    let threads = crate::Shared::new(std::sync::Mutex::new(Vec::new()));
    let given = closure.call_raw(context, None, [Dynamic::from(Scope(threads.clone()))]);
    let mut panicked = false;
    // A scoped thread may spawn more in the scope before it ends: each
    // round joins those the last left.
    loop {
        let spawned = std::mem::take(
            &mut *threads
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if spawned.is_empty() {
            break;
        }
        for handle in spawned {
            panicked |= matches!(handle.join(), Some(Err(_)));
        }
    }
    let value = given?;
    if panicked {
        return Err(
            EvalAltResult::ErrorRuntime("a scoped thread panicked".into(), Position::NONE).into(),
        );
    }
    Ok(value)
}

/// `builder.spawn(f)`: `f` on a thread std's `builder` starts, or the
/// error of one the system doesn't start.
#[cfg(feature = "sync")]
fn builder_spawn(
    context: Option<&NativeCallContext<'_>>,
    builder: &Dynamic,
    closure: Dynamic,
) -> RawrResult {
    let context = context.ok_or_else(|| invariant("a thread is spawned outside a call"))?;
    let closure = closure
        .try_cast::<crate::FnPtr>()
        .ok_or_else(|| invariant("a thread's closure is no function"))?;
    crate::std_facade::thread::spawn(builder, |std, name| {
        let started = context
            .global_runtime_state()
            .threads
            .start(context, closure, std, name)?;
        Ok(started.map(Dynamic::from))
    })
}

#[cfg(not(feature = "sync"))]
fn builder_spawn(_: Option<&NativeCallContext<'_>>, _: &Dynamic, _: Dynamic) -> RawrResult {
    Err(invariant(
        "`std::thread::Builder::spawn` needs rawr built with its `sync` feature",
    ))
}

#[cfg(not(feature = "sync"))]
fn scope(_: Option<&NativeCallContext<'_>>, _: Dynamic) -> RawrResult {
    Err(invariant(
        "`std::thread::scope` needs rawr built with its `sync` feature",
    ))
}

/// `s.spawn(f)`: `f` on a thread of its own, which the scope `s` joins.
#[cfg(feature = "sync")]
fn scoped_spawn(
    context: Option<&NativeCallContext<'_>>,
    scope: &Dynamic,
    closure: Dynamic,
) -> RawrResult {
    let threads = scope
        .read_lock::<Scope>()
        .map(|scope| scope.0.clone())
        .ok_or_else(|| invariant("a `Scope` is not one at runtime"))?;
    let handle = spawn(context, closure)?;
    let joined = handle
        .read_lock::<crate::eval::threads::Handle>()
        .map(|handle| handle.clone())
        .ok_or_else(|| invariant("a spawned thread gives no `JoinHandle`"))?;
    threads
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(joined);
    Ok(handle)
}

#[cfg(not(feature = "sync"))]
fn scoped_spawn(_: Option<&NativeCallContext<'_>>, _: &Dynamic, _: Dynamic) -> RawrResult {
    Err(invariant(
        "`std::thread::scope` needs rawr built with its `sync` feature",
    ))
}

#[cfg(feature = "sync")]
fn join(handle: &Dynamic) -> RawrResult {
    let handle = handle
        .read_lock::<crate::eval::threads::Handle>()
        .map(|handle| handle.clone())
        .ok_or_else(|| invariant("a `JoinHandle` is not one at runtime"))?;
    let outcome = handle
        .join()
        .ok_or_else(|| invariant("a `JoinHandle` joined twice"))?;
    Ok(match outcome {
        Ok(value) => crate::result::ok(value),
        // The thread printed its panic as it ended; `join` gives its
        // payload.
        Err(_) => crate::result::err(Dynamic::from(Panic)),
    })
}

#[cfg(not(feature = "sync"))]
fn join(_: &Dynamic) -> RawrResult {
    Err(invariant("a `JoinHandle` is not one at runtime"))
}

#[cfg(feature = "sync")]
fn is_finished(handle: &Dynamic) -> RawrResult {
    handle
        .read_lock::<crate::eval::threads::Handle>()
        .map(|handle| Dynamic::from(handle.is_finished()))
        .ok_or_else(|| invariant("a `JoinHandle` is not one at runtime"))
}

#[cfg(not(feature = "sync"))]
fn is_finished(_: &Dynamic) -> RawrResult {
    Err(invariant("a `JoinHandle` is not one at runtime"))
}

/// std's `Debug` of a thread or channel value.
fn debug(value: &Dynamic) -> String {
    if value.is::<Sender>() {
        "Sender { .. }".into()
    } else if value.is::<SyncSender>() {
        "SyncSender { .. }".into()
    } else if value.is::<Receiver>() {
        "Receiver { .. }".into()
    } else if value.is::<SendError>() {
        "SendError { .. }".into()
    } else if value.is::<Panic>() {
        "Any { .. }".into()
    } else {
        "JoinHandle { .. }".into()
    }
}

/// std's `Display` of a `SendError`, the one thread value it writes.
fn display() -> String {
    "sending on a closed channel".into()
}

impl Parser<'_> {
    /// `std::thread::spawn(f)`, after its path: `f` runs on a thread of its
    /// own, and the call is the thread's `JoinHandle<T>` of what `f` gives.
    pub(super) fn parse_thread_spawn(&mut self, start: Position) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("`spawn` arguments")?;
        let call = span(start, end);
        let [closure] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::thread", "spawn").to_string(),
                1,
                arguments.len(),
                call,
            ));
        };
        let output = self.spawned(closure, Spawner::Function, call)?;
        let source = self.prelude_op(
            PreludeOp::Thread,
            [closure.rendered_source(), code("spawn"), "()".into()],
        );
        Ok(self.operator_expr(source, Ty::Thread(Kind::JoinHandle, Box::new(output)), call))
    }

    /// The closure a thread runs, checked as std's `spawn` bounds it: `F:
    /// Send`, `T: Send`, and `F: 'static` unless it is a scope's, whose
    /// closure may borrow what outlives the scope. What the closure gives.
    fn spawned(
        &mut self,
        closure: &mut ParsedExpr,
        spawner: Spawner,
        call: Span,
    ) -> Result<Ty, Error> {
        let output = self.inference.fresh();
        let expected = Ty::Fn {
            inputs: Box::new([]),
            output: Box::new(output.clone()),
        };
        self.coerce_expr(&expected, closure)?;
        self.move_local(closure)?;
        // `F: 'static`: a closure that borrows a local may outlive it.
        if spawner != Spawner::Scope
            && let Some(name) = self.borrowed_captures.get(&closure.span)
        {
            return Err(Error::BorrowConflict {
                code: "E0373",
                message: format!(
                    "closure may outlive the current function, but it borrows `{name}`, which \
                     is owned by the current function"
                )
                .into(),
                access: closure.span,
                borrow: closure.span,
            });
        }
        // `F: Send` and `T: Send`, std's bounds.
        if let Some(unsent) = self.unsent_captures.get(&closure.span).cloned() {
            return Err(Error::MissingCapability {
                owner: unsent.ty.name(&self.models),
                capability: if unsent.borrowed { "Sync" } else { "Send" }.into(),
                operation: closure.span,
                derive: false,
                required_by: Some(spawner.path().into()),
            });
        }
        self.pending_bounds
            .push((output.clone(), capability::Kind::Send, call));
        Ok(output)
    }

    /// `std::thread::scope(f)`, after its path: `f` called with a
    /// `&Scope`, whose threads are joined before the call gives what `f`
    /// gave.
    pub(super) fn parse_thread_scope(&mut self, start: Position) -> Result<ParsedExpr, Error> {
        let scope = Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Thread(Kind::Scope, Box::new(Ty::Unit))),
        };
        // `F: FnOnce(&Scope) -> T`: the closure's parameter is the scope.
        self.closure_hints = vec![Some(Box::new([scope.clone()]))];
        let (mut arguments, end) = self.parse_call_arguments("`scope` arguments")?;
        let call = span(start, end);
        let [closure] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::thread", "scope").to_string(),
                1,
                arguments.len(),
                call,
            ));
        };
        let output = self.inference.fresh();
        let expected = Ty::Fn {
            inputs: Box::new([scope]),
            output: Box::new(output.clone()),
        };
        self.coerce_expr(&expected, closure)?;
        self.move_local(closure)?;
        let source = self.prelude_op(
            PreludeOp::Thread,
            [closure.rendered_source(), code("scope"), "()".into()],
        );
        let output = self.inference.resolve(&output);
        Ok(self.operator_expr(source, output, call))
    }

    /// `std::sync::mpsc::channel()`, or `sync_channel(bound)` when
    /// `bounded`, after its path: a `(Sender<T>, Receiver<T>)` of one
    /// channel, or a `(SyncSender<T>, Receiver<T>)`, `T` inferred from its
    /// uses.
    pub(super) fn parse_channel(
        &mut self,
        start: Position,
        bounded: Bound,
    ) -> Result<ParsedExpr, Error> {
        // `channel::<T>()`, whose turbofish a path's parse may have read.
        let item = if let Some(mut types) = self.pending_turbofish.take() {
            if types.len() != 1 {
                return Err(Error::InvalidTypeArguments {
                    ty: "function".into(),
                    expected: 1,
                    actual: types.len(),
                    reference: span(start, self.position()),
                });
            }
            types.remove(0)
        } else if matches!(self.peek(), Token::DoubleColon) {
            self.next();
            self.expect(Token::LessThan, "'<' after `channel::`")?;
            let item = self.parse_local_type()?;
            self.expect_close_angle("'>' after `channel`'s type argument")?;
            item
        } else {
            self.inference.fresh()
        };
        let (mut arguments, end) = self.parse_call_arguments("`channel` arguments")?;
        let call = span(start, end);
        let (function, sender) = match bounded {
            Bound::Unbounded => ("channel", Kind::Sender),
            Bound::Bounded => ("sync_channel", Kind::SyncSender),
        };
        let expected = usize::from(bounded == Bound::Bounded);
        if arguments.len() != expected {
            return Err(Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::sync::mpsc", function).to_string(),
                expected,
                arguments.len(),
                call,
            ));
        }
        let operand = match arguments.first_mut() {
            Some(bound) => {
                self.coerce_expr(&Ty::Int(Int::Usize), bound)?;
                bound.rendered_source()
            }
            None => "()".into(),
        };
        let ty = Ty::Tuple(Box::new([
            Ty::Thread(sender, Box::new(item.clone())),
            Ty::Thread(Kind::Receiver, Box::new(item)),
        ]));
        let source = self.prelude_op(PreludeOp::Thread, ["()".into(), code(function), operand]);
        Ok(self.operator_expr(source, ty, call))
    }

    /// Which of std's thread and channel types the path `path` names in a
    /// body, by its module path or as imported, unless the program names a
    /// type so: `thread::JoinHandle`, `mpsc::Receiver`.
    pub(super) fn thread_named(&self, path: &str) -> Option<Kind> {
        let named = |path: &str| crate::resolve::std_thread_type(path).and_then(Kind::named);
        let last = path.rsplit("::").next().unwrap_or(path);
        if self.models.get(last).is_some() {
            return None;
        }
        if path.contains("::") {
            return named(path);
        }
        self.models
            .import(path)
            .and_then(|binding| named(&binding.qualified))
    }

    /// `JoinHandle<T>`, `Sender<T>` or another of std's thread and channel
    /// types in a body, after its name; a lifetime before `T` is erased.
    pub(super) fn parse_thread_type(&mut self, kind: Kind) -> Result<Ty, Error> {
        self.expect(Token::LessThan, "'<' after the type's name")?;
        if self.skip_lifetime() {
            self.expect(Token::Comma, "',' after the type's lifetime")?;
        }
        let inner = self.parse_local_type()?;
        self.expect_close_angle("'>' after the type's argument")?;
        Ok(Ty::Thread(kind, Box::new(inner)))
    }

    /// Whether `ty` is std's `thread::Builder`, which Rawr's std holds.
    pub(super) fn is_thread_builder(&self, ty: &Ty) -> bool {
        let path = format!("{}::thread::Builder", crate::std_facade::ROOT);
        matches!(
            self.inference.resolve(ty),
            Ty::Host { proof, .. } if proof.definition.path().as_str() == path
        )
    }

    /// `builder.spawn(f)` of std's `thread::Builder`, after the method's
    /// name: `f` on a thread the builder names and sizes, as an
    /// `io::Result<JoinHandle<T>>`. `spawn(self, f)` takes the builder.
    pub(super) fn parse_builder_spawn(
        &mut self,
        mut builder: ParsedExpr,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("`spawn` arguments")?;
        let call = span(builder.span.start(), end);
        let [closure] = arguments.as_mut_slice() else {
            return Err(Error::argument_count(
                Callee::Method,
                crate::resolve::Qualified("Builder", "spawn").to_string(),
                1,
                arguments.len(),
                call,
            ));
        };
        let Some(error) = self.std_held("io::Error") else {
            return Err(Error::NotYet {
                path: "std::thread::Builder::spawn".into(),
                reference: member_span,
            });
        };
        self.move_local(&mut builder)?;
        let output = self.spawned(closure, Spawner::Builder, call)?;
        let source = self.prelude_op(
            PreludeOp::Thread,
            [
                builder.rendered_source(),
                code("builder_spawn"),
                closure.rendered_source(),
            ],
        );
        let handle = Ty::Thread(Kind::JoinHandle, Box::new(output));
        Ok(self.operator_expr(source, Ty::Result(Box::new(handle), Box::new(error)), call))
    }

    /// The error type Rawr's std holds as std's `sync::mpsc::{name}`, which
    /// a receive gives; the not-yet error of `method` without it.
    fn receive_error(&self, name: &str, method: &str, at: Span) -> Result<Ty, Error> {
        self.std_held(&format!("sync::mpsc::{name}"))
            .ok_or_else(|| Error::NotYet {
                path: format!("std::sync::mpsc::Receiver::{method}").into(),
                reference: at,
            })
    }

    /// A method of a `JoinHandle`, `Sender` or `Receiver` (a reference
    /// around it is the call's): std's, as rawr runs it, else the not-yet
    /// error for one std has and rustc's E0599 for one it hasn't.
    pub(super) fn parse_thread_member(
        &mut self,
        mut value: ParsedExpr,
        kind: Kind,
        inner: Ty,
        member: ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (mut arguments, end) = self.parse_call_arguments("method arguments")?;
        let call = span(value.span.start(), end);
        let owned = !matches!(self.inference.resolve(&value.ty), Ty::Reference { .. });
        let operand = |arguments: &[ParsedExpr]| {
            arguments
                .first()
                .map_or_else(|| "()".into(), ParsedExpr::rendered_source)
        };
        let error = |kind: Kind, inner: Ty| Ty::Thread(kind, Box::new(inner));
        let ty = match (kind, member.as_str(), arguments.len()) {
            // `join(self)` takes the handle.
            (Kind::JoinHandle | Kind::ScopedJoinHandle, "join", 0) => {
                if owned {
                    self.move_local(&mut value)?;
                }
                Ty::Result(Box::new(inner), Box::new(error(Kind::Panic, Ty::Unit)))
            }
            (Kind::JoinHandle | Kind::ScopedJoinHandle, "is_finished", 0) => Ty::Bool,
            (Kind::Sender | Kind::SyncSender, "send", 1) => {
                self.coerce_expr(&inner, &mut arguments[0])?;
                self.move_local(&mut arguments[0])?;
                Ty::Result(Box::new(Ty::Unit), Box::new(error(Kind::SendError, inner)))
            }
            (Kind::Sender | Kind::SyncSender, "clone", 0) => Ty::Thread(kind, Box::new(inner)),
            (Kind::Receiver, "recv", 0) => {
                let error = self.receive_error("RecvError", &member, member_span)?;
                Ty::Result(Box::new(inner), Box::new(error))
            }
            (Kind::Receiver, "try_recv", 0) => {
                let error = self.receive_error("TryRecvError", &member, member_span)?;
                Ty::Result(Box::new(inner), Box::new(error))
            }
            (Kind::Receiver, "recv_timeout", 1) => {
                self.coerce_expr(&Ty::Time(Time::Duration), &mut arguments[0])?;
                let error = self.receive_error("RecvTimeoutError", &member, member_span)?;
                Ty::Result(Box::new(inner), Box::new(error))
            }
            // `Scope::spawn(&'scope self, f)`: a thread the scope joins.
            (Kind::Scope, "spawn", 1) => {
                let output = self.spawned(&mut arguments[0], Spawner::Scope, call)?;
                let source = self.prelude_op(
                    PreludeOp::Thread,
                    [
                        value.rendered_source(),
                        code("scoped_spawn"),
                        operand(&arguments),
                    ],
                );
                let ty = Ty::Thread(Kind::ScopedJoinHandle, Box::new(output));
                return Ok(self.operator_expr(source, ty, call));
            }
            (Kind::Receiver, "iter", 0) => {
                let source = self.prelude_op(
                    PreludeOp::Thread,
                    [value.rendered_source(), code("iter"), "()".into()],
                );
                return Ok(self.received(source, inner, call));
            }
            _ => {
                let owner = Ty::Thread(kind, Box::new(inner.clone()));
                if let Some(found) = std_method::inherent(&owner, &member) {
                    return Err(Error::NotYet {
                        path: std_method::path(&found).into(),
                        reference: member_span,
                    });
                }
                return Err(Error::UnknownMember {
                    owner: owner.name(&self.models),
                    member,
                    reference: member_span,
                });
            }
        };
        let source = self.prelude_op(
            PreludeOp::Thread,
            [value.rendered_source(), code(&member), operand(&arguments)],
        );
        Ok(self.operator_expr(source, ty, call))
    }

    /// The values a receiver gives until every sender is gone, as an
    /// iterator of them: `rx.iter()`, and `for v in rx`.
    pub(super) fn received(
        &mut self,
        source: origin::Fragment,
        item: Ty,
        call: Span,
    ) -> ParsedExpr {
        let pipeline = self.source_pipeline(iterator::Source::Values {
            values: source,
            array: false,
        });
        self.iterator_expr(pipeline, item, call)
    }

    /// `{:?}` or `{}` of a thread or channel value, as std writes it.
    pub(super) fn thread_text(&mut self, value: origin::Fragment, debug: bool) -> origin::Fragment {
        let code = code(if debug { "debug" } else { "display" });
        self.prelude_op(PreludeOp::Thread, [value, code, "()".into()])
    }
}
