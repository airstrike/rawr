//! A simulated clock for headless recordings.
//!
//! While a clock is frozen on a thread, `Instant::now()` in scripts reads
//! it, `time::every` ticks by it, and `window::frames` follows the frames a
//! recording draws. A recording moves it forward one frame at a time, so the
//! same snippet records the same frames on every run. tokio's timers run on
//! a paused tokio clock that moves with it.

use std::cell::{Cell, RefCell};

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
            paused.runtime.block_on(tokio::time::advance(at - paused.at));
            paused.at = at;
        }
    });
}

/// The tokio runtime on this thread's frozen clock, made when first asked
/// for: one thread, its clock paused, moved only by [`set`].
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
            *tokio = Some(Paused { runtime, at: now });
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
