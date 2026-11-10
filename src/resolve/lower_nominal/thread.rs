//! `std::thread::spawn` and its `JoinHandle<T>`, and `std::sync::mpsc`'s
//! `channel`, `Sender<T>` and `Receiver<T>`. Each spawned closure runs on a
//! thread of its own, in parallel with the others (`eval::threads`), and a
//! channel is std's own, carrying rawr's values. Which methods each type
//! has is std's (`std_method`); how rawr runs each is here.
//!
//! A closure `spawn` takes must be `Send`, as std's bound says: one that
//! captures what rawr knows isn't `Send`, as an `Rc`, is rustc's E0277.

use super::*;

/// Which of std's thread and channel types a `Ty::Thread` is, with the
/// errors their methods give.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) enum Kind {
    JoinHandle,
    Sender,
    Receiver,
    /// `SendError<T>`, of a send no receiver takes.
    SendError,
    /// `RecvError`, of a receive no sender can answer; its `Ty::Thread`
    /// holds `()`.
    RecvError,
    /// `Box<dyn Any + Send>`, the panic `join` gives; its `Ty::Thread`
    /// holds `()`.
    Panic,
}

impl Kind {
    /// The kind rawr's builtin type `name` is, as `type_ref` names them.
    pub(super) fn named(name: &str) -> Option<Self> {
        Some(match name {
            "JoinHandle" => Self::JoinHandle,
            crate::resolve::MPSC_SENDER => Self::Sender,
            crate::resolve::MPSC_RECEIVER => Self::Receiver,
            _ => return None,
        })
    }

    /// The type's name, as rustc writes it with `inner`.
    pub(super) fn name(self, inner: &str) -> String {
        match self {
            Self::JoinHandle => format!("JoinHandle<{inner}>"),
            Self::Sender => format!("Sender<{inner}>"),
            Self::Receiver => format!("Receiver<{inner}>"),
            Self::SendError => format!("SendError<{inner}>"),
            Self::RecvError => "RecvError".into(),
            Self::Panic => "Box<dyn Any + Send>".into(),
        }
    }

    /// std's definition of the type, whose inherent methods and impls it
    /// has, and whether it takes the inner type as its argument. `None`
    /// for `join`'s `Box<dyn Any + Send>`, which is no std type rawr names.
    pub(super) const fn definition(self) -> Option<(&'static str, bool)> {
        Some(match self {
            Self::JoinHandle => ("std::thread::join_handle::JoinHandle", true),
            Self::Sender => ("std::sync::mpsc::Sender", true),
            Self::Receiver => ("std::sync::mpsc::Receiver", true),
            Self::SendError => ("std::sync::mpsc::SendError", true),
            Self::RecvError => ("std::sync::mpsc::RecvError", false),
            Self::Panic => return None,
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

    /// Whether std implements `Clone` for the kind: a `Sender` only.
    pub(super) const fn clones(self) -> bool {
        matches!(self, Self::Sender)
    }
}

/// The functions and methods rawr runs, by the code `invoke` takes.
const METHODS: [&str; 11] = [
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
];

fn code(name: &str) -> origin::Fragment {
    let code = METHODS
        .iter()
        .position(|method| *method == name)
        .expect("a listed thread method");
    code.to_string().into()
}

/// A channel's sending half, std's own.
#[derive(Clone)]
pub(crate) struct Sender(std::sync::mpsc::Sender<Dynamic>);

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
            let receiver = Receiver(crate::Shared::new(std::sync::Mutex::new(receiver)));
            Ok(Dynamic::from_array(vec![
                Dynamic::from(Sender(sender)),
                Dynamic::from(receiver),
            ]))
        }
        "send" => {
            let sender = args[0]
                .read_lock::<Sender>()
                .map(|sender| sender.0.clone())
                .ok_or_else(|| invariant("a `Sender` is not one at runtime"))?;
            Ok(match sender.send(args[2].take()) {
                Ok(()) => crate::result::ok(Dynamic::UNIT),
                Err(_) => crate::result::err(Dynamic::from(SendError)),
            })
        }
        // The receiver itself, which a `for` walks (`Receiver::values`).
        "iter" => Ok(args[0].clone()),
        "recv" | "try_recv" => {
            let receiver = args[0]
                .read_lock::<Receiver>()
                .map(|receiver| receiver.0.clone())
                .ok_or_else(|| invariant("a `Receiver` is not one at runtime"))?;
            let receiver = receiver
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(match METHODS[code] {
                "recv" => receiver
                    .recv()
                    .map_or_else(|_| crate::result::err(Dynamic::UNIT), crate::result::ok),
                _ => receiver
                    .try_recv()
                    .map_or_else(|_| crate::result::err(Dynamic::UNIT), crate::result::ok),
            })
        }
        "clone" => Ok(args[0].clone()),
        "debug" => Ok(Dynamic::from(ImmutableString::from(debug(args[0])))),
        "display" => Ok(Dynamic::from(ImmutableString::from(display(args[0])))),
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
    } else if value.is::<Receiver>() {
        "Receiver { .. }".into()
    } else if value.is::<SendError>() {
        "SendError { .. }".into()
    } else if value.is::<Panic>() {
        "Any { .. }".into()
    } else if value.is_unit() {
        // `RecvError`, whose runtime value is `()`.
        "RecvError".into()
    } else {
        "JoinHandle { .. }".into()
    }
}

/// std's `Display` of a channel's errors.
fn display(value: &Dynamic) -> String {
    if value.is::<SendError>() {
        "sending on a closed channel".into()
    } else {
        "receiving on a closed channel".into()
    }
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
        let output = self.inference.fresh();
        let expected = Ty::Fn {
            inputs: Box::new([]),
            output: Box::new(output.clone()),
        };
        self.coerce_expr(&expected, closure)?;
        self.move_local(closure)?;
        // `F: 'static`: a closure that borrows a local may outlive it.
        if let Some(name) = self.borrowed_captures.get(&closure.span) {
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
                owner: unsent.name(&self.models),
                capability: "Send".into(),
                operation: closure.span,
                derive: false,
                required_by: Some("std::thread::spawn".into()),
            });
        }
        self.pending_bounds
            .push((output.clone(), capability::Kind::Send, call));
        let source = self.prelude_op(
            PreludeOp::Thread,
            [closure.rendered_source(), code("spawn"), "()".into()],
        );
        Ok(self.operator_expr(source, Ty::Thread(Kind::JoinHandle, Box::new(output)), call))
    }

    /// `std::sync::mpsc::channel()`, after its path: a `(Sender<T>,
    /// Receiver<T>)` of one channel, `T` inferred from its uses.
    pub(super) fn parse_channel(&mut self, start: Position) -> Result<ParsedExpr, Error> {
        // `channel::<T>()`.
        let item = if matches!(self.peek(), Token::DoubleColon) {
            self.next();
            self.expect(Token::LessThan, "'<' after `channel::`")?;
            let item = self.parse_local_type()?;
            self.expect_close_angle("'>' after `channel`'s type argument")?;
            item
        } else {
            self.inference.fresh()
        };
        let (arguments, end) = self.parse_call_arguments("`channel` arguments")?;
        let call = span(start, end);
        if !arguments.is_empty() {
            return Err(Error::argument_count(
                Callee::Function,
                crate::resolve::Qualified("std::sync::mpsc", "channel").to_string(),
                0,
                arguments.len(),
                call,
            ));
        }
        let ty = Ty::Tuple(Box::new([
            Ty::Thread(Kind::Sender, Box::new(item.clone())),
            Ty::Thread(Kind::Receiver, Box::new(item)),
        ]));
        let source = self.prelude_op(
            PreludeOp::Thread,
            ["()".into(), code("channel"), "()".into()],
        );
        Ok(self.operator_expr(source, ty, call))
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
            (Kind::JoinHandle, "join", 0) => {
                if owned {
                    self.move_local(&mut value)?;
                }
                Ty::Result(Box::new(inner), Box::new(error(Kind::Panic, Ty::Unit)))
            }
            (Kind::JoinHandle, "is_finished", 0) => Ty::Bool,
            (Kind::Sender, "send", 1) => {
                self.coerce_expr(&inner, &mut arguments[0])?;
                self.move_local(&mut arguments[0])?;
                Ty::Result(Box::new(Ty::Unit), Box::new(error(Kind::SendError, inner)))
            }
            (Kind::Sender, "clone", 0) => Ty::Thread(Kind::Sender, Box::new(inner)),
            (Kind::Receiver, "recv" | "try_recv", 0) => {
                // `try_recv`'s error is `TryRecvError`, which rawr doesn't
                // name yet.
                if member.as_str() == "try_recv" {
                    return Err(Error::NotYet {
                        path: "std::sync::mpsc::Receiver::try_recv".into(),
                        reference: member_span,
                    });
                }
                Ty::Result(Box::new(inner), Box::new(error(Kind::RecvError, Ty::Unit)))
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
