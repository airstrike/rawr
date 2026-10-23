//! A simulated clock for headless recordings.
//!
//! While a clock is frozen on a thread, `Instant::now()` in scripts reads
//! it, `time::every` ticks by it, and `window::frames` follows the frames a
//! recording draws. A recording moves it forward one frame at a time, so the
//! same snippet records the same frames on every run.

use std::cell::Cell;

use iced::time::Instant;

thread_local! {
    static NOW: Cell<Option<Instant>> = const { Cell::new(None) };
    static ARMED: Cell<usize> = const { Cell::new(0) };
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
    }
}

/// The simulated time, when this thread's clock is frozen.
pub(crate) fn now() -> Option<Instant> {
    NOW.with(Cell::get)
}

/// Move the frozen clock to `at`.
pub(crate) fn set(at: Instant) {
    NOW.with(|now| now.set(Some(at)));
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
