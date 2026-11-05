mod cache;
mod chaining;
mod data_check;
mod debugger;
mod eval_context;
mod expr;
mod global_state;
pub(crate) mod machine;
mod stmt;
mod target;
#[cfg(feature = "sync")]
pub(crate) mod threads;

#[allow(unused_imports)]
pub use cache::FnResolutionCache;
pub use cache::{Caches, FnResolutionCacheEntry};
pub use data_check::calc_array_sizes;
pub use data_check::calc_data_sizes;
#[cfg(feature = "debugging")]
pub use debugger::CallStackFrame;
#[cfg(feature = "debugging")]
pub use debugger::{
    BreakPoint, BreakPositions, Debugger, DebuggerCommand, DebuggerEvent, DebuggerStatus,
    OnDebuggerCallback, OnDebuggingInit,
};
#[cfg(feature = "debugging")]
pub(crate) use debugger::{FrameBindings, Held};
pub use eval_context::{EvalContext, EvalContextFrameGuard};

pub use global_state::GlobalRuntimeState;
pub use global_state::SharedGlobalConstants;
pub use target::calc_offset_len;
pub use target::{calc_index, Target};
