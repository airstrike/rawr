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
use crate::{Dynamic, Engine, EvalAltResult, FnPtr, Position, Shared};

/// What a thread's closure gave: its value, or the panic that ended it.
pub type Outcome = Result<Dynamic, Box<EvalAltResult>>;

/// A thread's [`Outcome`] until it is joined.
type Slot = Shared<Mutex<Option<std::thread::JoinHandle<Outcome>>>>;

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
                    && let Ok(report) = crate::diagnostic::Report::from_evaluation(None, error)
                {
                    eprint!("{}", crate::diagnostic::panicked(&report, "<unnamed>"));
                }
                outcome
            })
            // What `std::thread::spawn` panics with.
            .map_err(|error| panic(&format!("failed to spawn thread: {error:?}")))?;
        let slot = Shared::new(Mutex::new(Some(thread)));
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
                if let Some(thread) = take(&slot) {
                    let _ = thread.join();
                }
            }
        }
    }
}

impl Handle {
    /// Wait for the thread to end, as `JoinHandle::join`: its closure's
    /// value, or the panic that ended it. A handle joins once; a copy
    /// joined after it finds nothing.
    pub(crate) fn join(&self) -> Option<Outcome> {
        let thread = take(&self.0)?;
        Some(
            thread
                .join()
                .unwrap_or_else(|_| Err(panic("rawr: a spawned thread aborted"))),
        )
    }

    /// Whether the thread's closure has returned, as
    /// `JoinHandle::is_finished`.
    pub(crate) fn is_finished(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }
}

fn take(slot: &Slot) -> Option<std::thread::JoinHandle<Outcome>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

fn panic(message: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}
