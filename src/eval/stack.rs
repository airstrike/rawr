//! The native stack a script call evaluates on.

/// The stack a script call's body may use before the calls it makes, the
/// most a debug build's evaluator frames take for one call level.
#[cfg(not(target_family = "wasm"))]
const ROOM: usize = 1 << 20;

/// The stack added when a call finds less than [`ROOM`] left.
#[cfg(not(target_family = "wasm"))]
const SEGMENT: usize = 8 << 20;

/// Run `body`, a script call's evaluation, with [`ROOM`] on the stack: on a
/// new segment when the host's thread has less left. The call-depth limit
/// is then what stops recursion, in a debug build as in a release one, on
/// any thread the host calls from.
#[inline(always)]
pub(crate) fn with_room<R>(body: impl FnOnce() -> R) -> R {
    #[cfg(not(target_family = "wasm"))]
    {
        stacker::maybe_grow(ROOM, SEGMENT, body)
    }
    #[cfg(target_family = "wasm")]
    {
        body()
    }
}
