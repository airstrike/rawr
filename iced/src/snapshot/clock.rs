//! A simulated clock for headless recordings.
//!
//! While a clock is frozen on a thread, `Instant::now()` in scripts reads
//! it, `time::every` ticks by it, and `window::frames` follows the frames a
//! recording draws. A recording moves it forward one frame at a time, so the
//! same snippet records the same frames on every run. tokio's timers run on
//! a paused tokio clock that moves with it. A plain program's tasks move it
//! straight to the next timer's end instead ([`advance`]), so a long sleep
//! costs no more than a short one.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use iced::time::Instant;

thread_local! {
    static NOW: Cell<Option<Instant>> = const { Cell::new(None) };
    static ARMED: Cell<usize> = const { Cell::new(0) };
    static TOKIO: RefCell<Option<Paused>> = const { RefCell::new(None) };
}

/// A tokio runtime whose clock is paused, and the frozen time it was last
/// moved to.
struct Paused {
    runtime: tokio::runtime::Runtime,
    at: Instant,
    fired: Fired,
}

/// Told when one of a program's timers on the paused clock ends, so
/// [`advance`] stops there.
#[derive(Clone, Default)]
pub(crate) struct Fired(Arc<tokio::sync::Notify>);

impl Fired {
    /// `waker`, which also tells [`advance`] that a timer ended.
    pub(crate) fn waker(&self, waker: &std::task::Waker) -> std::task::Waker {
        struct Both(std::task::Waker, Arc<tokio::sync::Notify>);
        impl std::task::Wake for Both {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.wake_by_ref();
                self.1.notify_waiters();
            }
        }
        std::task::Waker::from(Arc::new(Both(waker.clone(), self.0.clone())))
    }
}

/// The frozen clock on this thread; it thaws when dropped.
#[must_use = "the clock thaws when this is dropped"]
pub struct Frozen(());

/// Freeze this thread's clock at the present instant. Freeze it before an
/// application boots, so the times its boot reads are on the same clock.
pub fn freeze() -> Frozen {
    NOW.with(|now| now.set(Some(Instant::now())));
    Frozen(())
}

impl Drop for Frozen {
    fn drop(&mut self) {
        NOW.with(|now| now.set(None));
        TOKIO.with(|tokio| tokio.borrow_mut().take());
    }
}

/// Freeze this thread's clock at `at`: a `spawn_blocking` worker reads
/// the recording's time while the recording waits for it.
pub(crate) fn freeze_at(at: Instant) -> Frozen {
    NOW.with(|now| now.set(Some(at)));
    Frozen(())
}

/// The simulated time, when this thread's clock is frozen.
pub(crate) fn now() -> Option<Instant> {
    NOW.with(Cell::get)
}

/// Move the frozen clock to `at`. tokio's paused clock moves as far, so
/// its timers that end by then fire.
pub(crate) fn set(at: Instant) {
    NOW.with(|now| now.set(Some(at)));
    TOKIO.with(|tokio| {
        if let Some(paused) = tokio.borrow_mut().as_mut()
            && at > paused.at
        {
            paused
                .runtime
                .block_on(tokio::time::advance(at - paused.at));
            paused.at = at;
        }
    });
}

/// Move the frozen clock to the end of the next timer on tokio's paused
/// clock, at most `most` on, and end the timers due then. tokio's clock
/// goes there by itself while nothing else runs, so each sleep costs one
/// step however long it is.
pub(crate) fn advance(most: std::time::Duration) {
    let Some(now) = now() else {
        return;
    };
    let at = TOKIO.with(|tokio| {
        let mut tokio = tokio.borrow_mut();
        let Some(paused) = tokio.as_mut() else {
            return now + most;
        };
        let fired = paused.fired.0.clone();
        let moved = paused.runtime.block_on(async move {
            let start = tokio::time::Instant::now();
            let fired = std::pin::pin!(fired.notified());
            let most = std::pin::pin!(tokio::time::sleep(most));
            iced::futures::future::select(fired, most).await;
            start.elapsed()
        });
        paused.at += moved;
        paused.at
    });
    NOW.with(|now| now.set(Some(at)));
}

/// What tells [`advance`] that a timer on this thread's paused clock
/// ended, once that clock runs.
pub(crate) fn fired() -> Option<Fired> {
    TOKIO.with(|tokio| tokio.borrow().as_ref().map(|paused| paused.fired.clone()))
}

/// The tokio runtime on this thread's frozen clock, made when first asked
/// for: one thread, its clock paused, moved only by [`set`] and [`advance`].
pub(crate) fn tokio() -> Option<tokio::runtime::Handle> {
    let now = now()?;
    TOKIO.with(|tokio| {
        let mut tokio = tokio.borrow_mut();
        if tokio.is_none() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .ok()?;
            *tokio = Some(Paused {
                runtime,
                at: now,
                fired: Fired::default(),
            });
        }
        tokio.as_ref().map(|paused| paused.runtime.handle().clone())
    })
}

/// Whether a stream driven by the clock (`time::every`, `window::frames`)
/// is running.
pub(crate) fn armed() -> bool {
    ARMED.with(Cell::get) > 0
}

/// Held by a running stream that the clock drives.
pub(crate) struct Armed(());

impl Armed {
    pub(crate) fn new() -> Self {
        ARMED.with(|armed| armed.set(armed.get() + 1));
        Self(())
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        ARMED.with(|armed| armed.set(armed.get().saturating_sub(1)));
    }
}
