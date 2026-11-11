//! The threads a program starts with `std::thread::spawn`, each a thread
//! of the operating system that runs its closure in parallel with the
//! others, as Rust's do.
//!
//! A spawned thread evaluates with the engine of the evaluation that
//! started it. Every evaluation the engine begins has a fresh
//! [`GlobalRuntimeState`](super::GlobalRuntimeState), whose [`Threads`]
//! is the root of its group: the calls under it share the group, and the
//! threads they spawn join it. The root joins every thread of its group
//! that no one joined before it drops, as `std::thread::scope` joins its
//! own. So no thread outlives the evaluation, which borrows the engine
//! for its whole length, and each may use the engine while it runs.
//!
//! A Rust program exits when `main` returns, ending the threads still
//! running. An engine that lasts for the process ([`Engine::leak`], as
//! `rawr run`'s does) leaves its threads running as its evaluation
//! returns, and the process ends them as it exits.

use std::sync::{Mutex, PoisonError};

use crate::func::NativeCallContext;
use crate::host::runtime::Witness;
use crate::{Dynamic, Engine, EvalAltResult, FnPtr, Position, Shared};

/// What a thread's closure gave: its value, or the panic that ended it.
pub type Outcome = Result<Dynamic, Box<EvalAltResult>>;

/// A thread's [`Outcome`] until it is joined.
type Slot = Shared<Mutex<Option<Thread>>>;

/// A thread a handle joins.
enum Thread {
    /// One the program spawned, which runs its closure.
    Program(std::thread::JoinHandle<Outcome>),
    /// One a crate spawned and gave the `JoinHandle` of (#1266).
    Crate(Box<dyn Joined>),
    /// One the program spawned whose handle a crate took (#1274): the
    /// thread that joins it for the crate drops the sender of `ended` once
    /// it has.
    Lent(std::sync::mpsc::Receiver<()>),
}

/// A crate's thread, which gives its value as rawr's once it is joined.
trait Joined: Send {
    fn join(self: Box<Self>) -> Outcome;
    fn is_finished(&self) -> bool;
    /// Take the witness of the thread's `T`, which the value it gives is
    /// attested with as it becomes rawr's (#1275).
    fn attest(&mut self, witness: Witness);
    /// The crate's own `JoinHandle<T>`, as a crate takes it back.
    fn into_handle(self: Box<Self>) -> Box<dyn std::any::Any + Send>;
}

/// A crate's `JoinHandle<T>`, what makes its `T` rawr's, and the witness
/// of `T` the call that gave it had.
struct Native<T, F> {
    handle: std::thread::JoinHandle<T>,
    give: F,
    witness: Option<Witness>,
}

impl<T, F> Joined for Native<T, F>
where
    T: Send + 'static,
    F: FnOnce(T, Option<&Witness>) -> Outcome + Send,
{
    fn join(self: Box<Self>) -> Outcome {
        let Self {
            handle,
            give,
            witness,
        } = *self;
        // The thread printed its panic as it ended, through Rust's hook.
        handle
            .join()
            .map_err(|_| panic("a crate's thread panicked"))
            .and_then(|value| give(value, witness.as_ref()))
    }

    fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    fn attest(&mut self, witness: Witness) {
        self.witness = Some(witness);
    }

    fn into_handle(self: Box<Self>) -> Box<dyn std::any::Any + Send> {
        Box::new(self.handle)
    }
}

/// The threads one evaluation spawned, and whether its root still runs.
#[derive(Default)]
struct Group {
    /// `None` once the root has dropped.
    running: Mutex<Option<Vec<Slot>>>,
}

/// An evaluation's share of its thread group: the root, made with the
/// evaluation's state, or a copy of it, which a call under it holds.
pub struct Threads {
    group: Shared<Group>,
    /// Whether this is the root, which joins the group as it drops.
    root: bool,
}

/// A spawned thread, as Rust's `JoinHandle` is: joined at most once.
#[derive(Clone)]
pub struct Handle(Slot);

impl Threads {
    /// The root of a new evaluation's group, which joins it unless the
    /// engine is `lasting`: one that lives as long as the process.
    pub(crate) fn root(lasting: bool) -> Self {
        Self {
            group: Shared::new(Group {
                running: Mutex::new(Some(Vec::new())),
            }),
            root: !lasting,
        }
    }

    /// Spawn a thread that calls `closure` with no arguments in the
    /// evaluation `context` belongs to, as `std::thread::spawn(closure)`.
    pub(crate) fn spawn(
        &self,
        context: &NativeCallContext<'_>,
        closure: FnPtr,
    ) -> Result<Handle, Box<EvalAltResult>> {
        let mut running = self
            .group
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Only a store of a call's context outlives its evaluation.
        let Some(running) = running.as_mut() else {
            return Err(panic(
                "rawr: a thread was spawned after its evaluation returned",
            ));
        };
        // SAFETY: the engine lasts for the process, or the group's root
        // joins this thread before it drops (its `Drop`, which waits for
        // this lock to add the thread first). The root belongs to the state
        // of an evaluation of this engine, which borrows the engine until it
        // returns, after the root drops. So the engine outlives every use
        // the thread makes of it.
        let engine: &'static Engine = unsafe { &*std::ptr::from_ref(context.engine()) };
        let global = context.global_runtime_state().clone();
        let name = context.fn_name().to_owned();
        let position = context.call_position();
        let thread = std::thread::Builder::new()
            .spawn(move || {
                let context = NativeCallContext::from((engine, &*name, None, &global, position));
                let outcome = closure.call_raw(&context, None, []);
                // A thread that panics prints its panic as it ends, as
                // Rust's panic hook does; `join` gives the payload.
                if let Err(error) = &outcome
                    && let Some(report) = crate::diagnostic::located(error)
                {
                    eprint!("{}", crate::diagnostic::panicked(&report, "<unnamed>"));
                }
                outcome
            })
            // What `std::thread::spawn` panics with.
            .map_err(|error| panic(&format!("failed to spawn thread: {error:?}")))?;
        let slot = Shared::new(Mutex::new(Some(Thread::Program(thread))));
        running.push(slot.clone());
        Ok(Handle(slot))
    }
}

impl Clone for Threads {
    /// A call's copy, which the root's group outlives.
    fn clone(&self) -> Self {
        Self {
            group: self.group.clone(),
            root: false,
        }
    }
}

impl Drop for Threads {
    fn drop(&mut self) {
        if !self.root {
            return;
        }
        // A thread joined here may spawn more before it ends: each round
        // joins those the last left.
        loop {
            let slots = {
                let mut running = self
                    .group
                    .running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                match running.as_mut() {
                    Some(slots) if !slots.is_empty() => std::mem::take(slots),
                    _ => {
                        *running = None;
                        return;
                    }
                }
            };
            for slot in slots {
                match take(&slot) {
                    Some(Thread::Program(thread)) => {
                        let _ = thread.join();
                    }
                    // A crate took it: the thread that joins it for the
                    // crate says when it has.
                    Some(Thread::Lent(ended)) => {
                        let _ = ended.recv();
                    }
                    Some(Thread::Crate(_)) | None => {}
                }
            }
        }
    }
}

impl Handle {
    /// The handle of a crate's thread, as a crate's function gives its
    /// `JoinHandle<T>`: `give` makes the thread's `T` rawr's where it is
    /// joined. No group joins it, as no Rust scope joins a crate's thread.
    pub(crate) fn native<T, F>(handle: std::thread::JoinHandle<T>, give: F) -> Self
    where
        T: Send + 'static,
        F: FnOnce(T, Option<&Witness>) -> Outcome + Send + 'static,
    {
        Self(Shared::new(Mutex::new(Some(Thread::Crate(Box::new(
            Native {
                handle,
                give,
                witness: None,
            },
        ))))))
    }

    /// Give a crate's thread the witness of its `T`, as the call that gave
    /// its handle declares it (#1275).
    pub(crate) fn attest(&self, witness: Witness) {
        if let Some(Thread::Crate(thread)) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            thread.attest(witness);
        }
    }

    /// Wait for the thread to end, as `JoinHandle::join`: its closure's
    /// value, or the panic that ended it. A handle joins once; a copy
    /// joined after it finds nothing.
    pub(crate) fn join(&self) -> Option<Outcome> {
        Some(match take(&self.0)? {
            Thread::Program(thread) => thread
                .join()
                .unwrap_or_else(|_| Err(panic("rawr: a spawned thread aborted"))),
            Thread::Crate(thread) => thread.join(),
            // A crate holds it, as Rust's moved handle is.
            Thread::Lent(ended) => {
                restore(&self.0, Thread::Lent(ended));
                return None;
            }
        })
    }

    /// The `JoinHandle<T>` a crate takes of this handle (#1274): the
    /// crate's own, as it gave it, or one of a thread that joins the
    /// program's and makes what its closure gave a `T` with `make`. A
    /// panic of the program's thread, or a value `make` refuses, is the
    /// panic the crate's `join` gives. `None` once it is joined, or for
    /// a crate's of another `T`.
    pub(crate) fn take_native<T, F>(&self, make: F) -> Option<std::thread::JoinHandle<T>>
    where
        T: Send + 'static,
        F: FnOnce(Dynamic) -> Result<T, String> + Send + 'static,
    {
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match slot.take()? {
            Thread::Crate(thread) => thread
                .into_handle()
                .downcast::<std::thread::JoinHandle<T>>()
                .ok()
                .map(|handle| *handle),
            // The group's root still waits for the program's thread, which
            // borrows the engine, through `ended`.
            Thread::Program(thread) => {
                let (done, ended) = std::sync::mpsc::channel::<()>();
                *slot = Some(Thread::Lent(ended));
                std::thread::Builder::new()
                    .spawn(move || {
                        let value = match thread.join() {
                            Ok(Ok(value)) => make(value),
                            // The thread printed its panic as it ended.
                            Ok(Err(_)) | Err(_) => Err("a program's thread panicked".to_owned()),
                        };
                        drop(done);
                        // A payload, as the program's panic already printed.
                        value.unwrap_or_else(|message| std::panic::resume_unwind(Box::new(message)))
                    })
                    .ok()
            }
            lent @ Thread::Lent(_) => {
                *slot = Some(lent);
                None
            }
        }
    }

    /// Whether the thread's closure has returned, as
    /// `JoinHandle::is_finished`.
    pub(crate) fn is_finished(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_none_or(|thread| match thread {
                Thread::Program(thread) => thread.is_finished(),
                Thread::Crate(thread) => thread.is_finished(),
                Thread::Lent(ended) => matches!(
                    ended.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Disconnected)
                ),
            })
    }
}

fn take(slot: &Slot) -> Option<Thread> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

fn restore(slot: &Slot, thread: Thread) {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(thread);
}

fn panic(message: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}
