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

#[allow(unused_imports)]
pub use cache::FnResolutionCache;
pub use cache::{Caches, FnResolutionCacheEntry};
pub use data_check::calc_array_sizes;
pub use data_check::calc_data_sizes;
#[cfg(feature = "debugging")]
pub use debugger::CallStackFrame;
#[cfg(feature = "debugging")]
pub use debugger::{
    BreakPoint, Debugger, DebuggerCommand, DebuggerEvent, DebuggerStatus, OnDebuggerCallback,
    OnDebuggingInit,
};
pub use eval_context::{EvalContext, EvalContextFrameGuard};

pub use global_state::GlobalRuntimeState;
pub use global_state::SharedGlobalConstants;
pub(crate) use stmt::restore_nominal_root;
pub use target::calc_offset_len;
pub use target::{calc_index, Target};
