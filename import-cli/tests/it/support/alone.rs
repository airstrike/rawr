//! Which tests run beside others. The suite runs its tests in parallel, and
//! a test shares the machine from its first use of [`super::scratch`] to
//! its end. A test that times a live program, whose windows mean nothing
//! while a dozen builds load the machine, calls [`alone`] before it starts
//! timing: it waits for the other tests running then to end, and no other
//! test starts until it ends. Its builds before that point run beside the
//! others'. Each test runs on a thread of its own, which holds its turn
//! until the thread ends; run on one thread (`--test-threads=1`), tests
//! take turns anyway, and none is held.

use std::cell::RefCell;
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// The suite's turns: shared by tests that run beside others, whole for one
/// that runs alone.
static TURNS: RwLock<()> = RwLock::new(());

/// The turn the current test's thread holds.
enum Turn {
    Shared {
        _held: RwLockReadGuard<'static, ()>,
    },
    Whole {
        _held: RwLockWriteGuard<'static, ()>,
    },
}

thread_local! {
    static HELD: RefCell<Option<Turn>> = const { RefCell::new(None) };
}

/// Run the rest of the current test alone: wait for the other tests running
/// to end, and hold every other test back until this one ends.
pub fn alone() {
    if !on_test_thread() {
        return;
    }
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        // Its shared turn goes first, or the whole one would wait on it.
        held.take();
        *held = Some(Turn::Whole {
            _held: TURNS.write().unwrap_or_else(PoisonError::into_inner),
        });
    });
}

/// Run the current test beside others, unless it already holds a turn.
pub(super) fn share() {
    if !on_test_thread() {
        return;
    }
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if held.is_none() {
            *held = Some(Turn::Shared {
                _held: TURNS.read().unwrap_or_else(PoisonError::into_inner),
            });
        }
    });
}

/// Whether this is a test's own thread, which ends with the test: libtest
/// names it for the test, and runs tests on its main thread only when it
/// runs them one at a time.
fn on_test_thread() -> bool {
    std::thread::current()
        .name()
        .is_some_and(|name| name != "main")
}
