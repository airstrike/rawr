//! Module defining the debugging interface.
#![cfg(feature = "debugging")]

use super::{Caches, EvalContext, GlobalRuntimeState};
use crate::ast::{ASTNode, Expr, Stmt};
use crate::{Dynamic, Engine, EvalAltResult, ImmutableString, Position, RawrResultOf, Scope};
use std::{fmt, mem};

/// Callback function to initialize the debugger.
#[cfg(not(feature = "sync"))]
pub type OnDebuggingInit = dyn Fn(&Engine, Debugger) -> Debugger;
/// Callback function to initialize the debugger.
#[cfg(feature = "sync")]
pub type OnDebuggingInit = dyn Fn(&Engine, Debugger) -> Debugger + Send + Sync;

/// Callback function for debugging.
#[cfg(not(feature = "sync"))]
pub type OnDebuggerCallback = dyn Fn(
    EvalContext,
    DebuggerEvent,
    ASTNode,
    Option<&str>,
    Position,
) -> RawrResultOf<DebuggerCommand>;
/// Callback function for debugging.
#[cfg(feature = "sync")]
pub type OnDebuggerCallback = dyn Fn(EvalContext, DebuggerEvent, ASTNode, Option<&str>, Position) -> RawrResultOf<DebuggerCommand>
    + Send
    + Sync;

/// A command for the debugger on the next iteration.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Hash)]
#[non_exhaustive]
pub enum DebuggerCommand {
    /// Continue normal execution.
    #[default]
    Continue,
    /// Step into the next expression, diving into functions.
    StepInto,
    /// Run to the next expression or statement, stepping over functions.
    StepOver,
    /// Run to the next statement, skipping over functions.
    Next,
    /// Run to the end of the current function call.
    FunctionExit,
}

/// The debugger status.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum DebuggerStatus {
    // Script evaluation starts.
    Init,
    // Stop at the next statement or expression.
    Next(bool, bool),
    // Run to the end of the current level of function call.
    FunctionExit(usize),
    // Script evaluation ends.
    Terminate,
}

impl DebuggerStatus {
    pub const CONTINUE: Self = Self::Next(false, false);
    pub const STEP: Self = Self::Next(true, true);
    pub const NEXT: Self = Self::Next(true, false);
    pub const INTO: Self = Self::Next(false, true);
}

/// A event that triggers the debugger.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum DebuggerEvent<'a> {
    /// Script evaluation starts.
    Start,
    /// Break on next step.
    Step,
    /// Break on break-point.
    BreakPoint(usize),
    /// Return from a function with a value.
    FunctionExitWithValue(&'a Dynamic),
    /// Return from a function with a value.
    FunctionExitWithError(&'a EvalAltResult),
    /// Script evaluation ends.
    End,
    /// Another thread woke the debugger ([`Engine::wake_debugger_on`]).
    Wake,
    /// A step at one of the engine's break positions
    /// ([`Engine::break_debugger_at`]).
    Break,
}

/// Positions in a program where the debugger callback runs, which another
/// thread may change while the program runs ([`Engine::break_debugger_at`]).
/// While it holds none, a step reads one flag.
#[derive(Debug, Default)]
pub struct BreakPositions {
    any: std::sync::atomic::AtomicBool,
    positions: std::sync::RwLock<std::collections::HashSet<Position>>,
}

impl BreakPositions {
    /// Break at `positions` from now on, in place of the ones before.
    pub fn set(&self, positions: impl IntoIterator<Item = Position>) {
        let mut held = self
            .positions
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        held.clear();
        held.extend(positions);
        self.any
            .store(!held.is_empty(), std::sync::atomic::Ordering::Release);
    }

    /// Whether a step at `position` breaks.
    #[must_use]
    pub fn contains(&self, position: Position) -> bool {
        self.any.load(std::sync::atomic::Ordering::Acquire)
            && self
                .positions
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains(&position)
    }
}

/// A break-point for debugging.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum BreakPoint {
    /// Break at a particular position under a particular source.
    AtPosition {
        /// Source (empty if not available) of the break-point.
        source: Option<ImmutableString>,
        /// [Position] of the break-point.
        pos: Position,
        /// Is the break-point enabled?
        enabled: bool,
    },
    /// Break at a particular function call.
    AtFunctionName {
        /// Function name.
        name: ImmutableString,
        /// Is the break-point enabled?
        enabled: bool,
    },
    /// Break at a particular function call with a particular number of arguments.
    AtFunctionCall {
        /// Function name.
        name: ImmutableString,
        /// Number of arguments.
        args: usize,
        /// Is the break-point enabled?
        enabled: bool,
    },
    /// Break at a particular property .
    AtProperty {
        /// Property name.
        name: ImmutableString,
        /// Is the break-point enabled?
        enabled: bool,
    },
}

impl fmt::Display for BreakPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtPosition {
                source,
                pos,
                enabled,
            } => {
                if let Some(source) = source {
                    write!(f, "{source} ")?;
                }
                write!(f, "@ {pos:?}")?;
                if !*enabled {
                    f.write_str(" (disabled)")?;
                }
                Ok(())
            }
            Self::AtFunctionName { name, enabled } => {
                write!(f, "{name} (...)")?;
                if !*enabled {
                    f.write_str(" (disabled)")?;
                }
                Ok(())
            }
            Self::AtFunctionCall {
                name,
                args,
                enabled,
            } => {
                write!(
                    f,
                    "{name} ({})",
                    std::iter::repeat_n("_", *args)
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
                if !*enabled {
                    f.write_str(" (disabled)")?;
                }
                Ok(())
            }
            Self::AtProperty { name, enabled } => {
                write!(f, ".{name}")?;
                if !*enabled {
                    f.write_str(" (disabled)")?;
                }
                Ok(())
            }
        }
    }
}

impl BreakPoint {
    /// Is this [`BreakPoint`] enabled?
    #[inline(always)]
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        match self {
            Self::AtPosition { enabled, .. } => *enabled,
            Self::AtFunctionName { enabled, .. } | Self::AtFunctionCall { enabled, .. } => *enabled,
            Self::AtProperty { enabled, .. } => *enabled,
        }
    }
    /// Enable/disable this [`BreakPoint`].
    #[inline(always)]
    pub fn enable(&mut self, value: bool) {
        match self {
            Self::AtPosition { enabled, .. } => *enabled = value,
            Self::AtFunctionName { enabled, .. } | Self::AtFunctionCall { enabled, .. } => {
                *enabled = value
            }
            Self::AtProperty { enabled, .. } => *enabled = value,
        }
    }
}

/// A function call.
#[derive(Debug, Clone, Hash)]
#[non_exhaustive]
pub struct CallStackFrame {
    /// Function name.
    pub fn_name: ImmutableString,
    /// Source of the function.
    pub source: Option<ImmutableString>,
    /// [Position][`Position`] of the function call.
    pub pos: Position,
    /// A script function's bindings: where they start in its scope and its
    /// receiver's name (#511).
    pub(crate) bindings: Option<FrameBindings>,
    /// Where the caller keeps its own bindings while this call runs, as the
    /// caller's call site lent them (#540).
    pub(crate) caller: Option<Lent>,
}

/// A script function's bindings: its parameters went into its scope from
/// `start`, and its receiver is the one the program named `this_name` (a
/// `&mut` parameter or `self`, #534).
#[derive(Debug, Clone, Hash)]
pub(crate) struct FrameBindings {
    pub(crate) start: usize,
    pub(crate) this_name: Option<ImmutableString>,
}

/// What a call site lends the call it makes, so the debugger can read the
/// caller's bindings while the call runs (#540).
///
/// The call site takes these pointers from its own borrows of its scope
/// and receiver after its last use of them before the call, and uses
/// neither again until the call returns. They are the most recent
/// borrows, so reading through them while the call runs aliases no live
/// `&mut`. The one place the call holds by reference, `held`, is never
/// read through them. They are read only on the thread that runs the
/// call, while its frame is on the call stack, by the debugger callback
/// of a step it led to (`crate::inspect`). A copy of the call stack that
/// can outlive the calls (`NativeCallContext::store_data`) drops them.
#[derive(Debug, Clone, Hash)]
pub(crate) struct Lent {
    /// The caller's scope, or `None` when the call extends it.
    pub(crate) scope: Option<std::ptr::NonNull<Scope<'static>>>,
    /// The caller's receiver, when the call doesn't hold it.
    pub(crate) this: Option<std::ptr::NonNull<Dynamic>>,
    /// The caller's binding the call holds by reference: its receiver, or
    /// the local at this address. Never dereferenced.
    pub(crate) held: Option<Held>,
}

/// A caller's binding a call holds by reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Held {
    /// The caller's receiver.
    This,
    /// The caller's local whose value is at this address.
    Local(*const Dynamic),
}

// SAFETY: the pointers are dereferenced only on the thread that runs the
// call they were lent to, while it runs (see `Lent`).
unsafe impl Send for Lent {}
// SAFETY: as for `Send`.
unsafe impl Sync for Lent {}

impl fmt::Display for CallStackFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.fn_name)?;

        if !self.pos.is_none() {
            if let Some(ref source) = self.source {
                write!(f, ": {source}")?;
            }
            write!(f, " @ {:?}", self.pos)?;
        }

        Ok(())
    }
}

/// A type providing debugging facilities.
#[derive(Debug, Clone, Hash)]
pub struct Debugger {
    /// The current status command.
    pub(crate) status: DebuggerStatus,
    /// The current set of break-points.
    break_points: Vec<BreakPoint>,
    /// The current function call stack.
    call_stack: Vec<CallStackFrame>,
    /// What the call about to be made was lent, for its frame.
    lent: Option<Lent>,
    /// The current state.
    state: Dynamic,
}

impl Debugger {
    /// Create a new [`Debugger`].
    #[inline(always)]
    #[must_use]
    pub fn new(status: DebuggerStatus) -> Self {
        Self {
            status,
            break_points: Vec::new(),
            call_stack: Vec::new(),
            lent: None,
            state: Dynamic::UNIT,
        }
    }
    /// Get the current call stack.
    #[inline(always)]
    #[must_use]
    pub fn call_stack(&self) -> &[CallStackFrame] {
        &self.call_stack
    }
    /// Rewind the function call stack to a particular depth.
    #[inline(always)]
    pub(crate) fn rewind_call_stack(&mut self, len: usize) {
        self.call_stack.truncate(len);
    }
    /// Lend the call about to be made where the caller keeps its
    /// bindings. The call's frame takes it; [`Self::unlend`] drops it when
    /// the call made no frame.
    #[inline(always)]
    pub(crate) fn lend(&mut self, lent: Lent) {
        self.lent = Some(lent);
    }
    /// Drop what [`Self::lend`] lent, once the call it was for returned.
    #[inline(always)]
    pub(crate) fn unlend(&mut self) {
        self.lent = None;
    }
    /// Add a native function's frame: its name and position only (#513).
    /// `holds_first` is whether the call holds its first argument by
    /// reference.
    #[inline(always)]
    pub(crate) fn push_call_stack_frame(
        &mut self,
        fn_name: ImmutableString,
        source: Option<ImmutableString>,
        pos: Position,
        holds_first: bool,
    ) {
        let caller = self.take_lent(holds_first);
        self.call_stack.push(CallStackFrame {
            fn_name,
            source,
            pos,
            bindings: None,
            caller,
        });
    }
    /// Add a script function's frame. Its bindings stay where its call
    /// keeps them, and are read there. `holds_this` is whether the call
    /// holds a receiver by reference.
    #[inline(always)]
    pub(crate) fn push_script_frame(
        &mut self,
        fn_name: ImmutableString,
        source: Option<ImmutableString>,
        pos: Position,
        bindings: FrameBindings,
        holds_this: bool,
    ) {
        let caller = self.take_lent(holds_this);
        self.call_stack.push(CallStackFrame {
            fn_name,
            source,
            pos,
            bindings: Some(bindings),
            caller,
        });
    }
    /// What the call being pushed was lent. A local it was lent by
    /// reference stays held only when the call holds it, not a copy. A
    /// receiver stays held either way: the call site lent no pointer to it.
    fn take_lent(&mut self, holds: bool) -> Option<Lent> {
        let mut lent = self.lent.take()?;
        if !holds && matches!(lent.held, Some(Held::Local(..))) {
            lent.held = None;
        }
        Some(lent)
    }
    /// Forget where each caller keeps its bindings, for a copy of this
    /// debugger that can outlive the calls.
    pub(crate) fn forget_bindings(&mut self) {
        self.lent = None;
        for frame in &mut self.call_stack {
            frame.caller = None;
        }
    }
    /// Change the current status to [`CONTINUE`][DebuggerStatus::CONTINUE] and return the previous status.
    pub(crate) fn clear_status_if(
        &mut self,
        filter: impl FnOnce(&DebuggerStatus) -> bool,
    ) -> Option<DebuggerStatus> {
        if filter(&self.status) {
            Some(mem::replace(&mut self.status, DebuggerStatus::CONTINUE))
        } else {
            None
        }
    }
    /// Override the status of this [`Debugger`] if the current status is
    /// [`CONTINUE`][DebuggerStatus::CONTINUE].
    #[inline(always)]
    pub(crate) fn reset_status(&mut self, status: DebuggerStatus) {
        if self.status == DebuggerStatus::CONTINUE {
            self.status = status;
        }
    }
    /// Returns the first break-point triggered by a particular [`AST` Node][ASTNode].
    #[must_use]
    pub fn is_break_point(&self, src: Option<&str>, node: ASTNode) -> Option<usize> {
        let _src = src;

        self.break_points()
            .iter()
            .enumerate()
            .filter(|(.., bp)| bp.is_enabled())
            .find(|(.., bp)| match bp {
                BreakPoint::AtPosition { pos, .. } if pos.is_none() => false,
                BreakPoint::AtPosition { source, pos, .. } if pos.is_beginning_of_line() => {
                    node.position().line().unwrap_or(0) == pos.line().unwrap()
                        && _src == source.as_deref()
                }
                BreakPoint::AtPosition { source, pos, .. } => {
                    node.position() == *pos && _src == source.as_deref()
                }
                BreakPoint::AtFunctionName { name, .. } => match node {
                    ASTNode::Expr(Expr::FnCall(x, ..)) | ASTNode::Stmt(Stmt::FnCall(x, ..)) => {
                        x.name == *name
                    }
                    ASTNode::Stmt(Stmt::Expr(e)) => match &**e {
                        Expr::FnCall(x, ..) => x.name == *name,
                        _ => false,
                    },
                    _ => false,
                },
                BreakPoint::AtFunctionCall { name, args, .. } => match node {
                    ASTNode::Expr(Expr::FnCall(x, ..)) | ASTNode::Stmt(Stmt::FnCall(x, ..)) => {
                        x.args.len() == *args && x.name == *name
                    }
                    ASTNode::Stmt(Stmt::Expr(e)) => match &**e {
                        Expr::FnCall(x, ..) => x.args.len() == *args && x.name == *name,
                        _ => false,
                    },
                    _ => false,
                },
                BreakPoint::AtProperty { name, .. } => match node {
                    ASTNode::Expr(Expr::Property(x, ..)) => x.2 == *name,
                    _ => false,
                },
            })
            .map(|(i, ..)| i)
    }
    /// Get a slice of all [`BreakPoint`]'s.
    #[inline(always)]
    #[must_use]
    pub fn break_points(&self) -> &[BreakPoint] {
        &self.break_points
    }
    /// Get the underlying [`Vec`] holding all [`BreakPoint`]'s.
    #[inline(always)]
    #[must_use]
    pub fn break_points_mut(&mut self) -> &mut Vec<BreakPoint> {
        &mut self.break_points
    }
    /// Get the custom state.
    #[inline(always)]
    pub const fn state(&self) -> &Dynamic {
        &self.state
    }
    /// Get a mutable reference to the custom state.
    #[inline(always)]
    pub fn state_mut(&mut self) -> &mut Dynamic {
        &mut self.state
    }
    /// Set the custom state.
    #[inline(always)]
    pub fn set_state(&mut self, state: impl Into<Dynamic>) {
        self.state = state.into();
    }
}

impl Engine {
    /// Lend the call about to be made where its caller keeps its bindings
    /// (#540), while a debugger is registered: `scope`, or `None` when the
    /// call extends the caller's scope, its receiver `this`, and the place
    /// the call is given by reference, `held`. The caller passes its most
    /// recent borrows and uses neither again until the call returns, then
    /// calls [`Self::unlend_bindings`].
    #[inline(always)]
    pub(crate) fn lend_bindings(
        &self,
        global: &mut GlobalRuntimeState,
        scope: Option<std::ptr::NonNull<Scope>>,
        this: Option<std::ptr::NonNull<Dynamic>>,
        held: Option<Held>,
    ) {
        if self.is_debugger_registered() {
            global.debugger_mut().lend(Lent {
                scope: scope.map(std::ptr::NonNull::cast),
                this,
                held,
            });
        }
    }
    /// Drop what [`Self::lend`] lent, once the call returned.
    #[inline(always)]
    pub(crate) fn unlend_bindings(&self, global: &mut GlobalRuntimeState) {
        if self.is_debugger_registered() {
            global.debugger_mut().unlend();
        }
    }
    /// Run the debugger callback if there is a debugging interface registered.
    #[inline(always)]
    pub(crate) fn dbg<'a>(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        this_ptr: Option<&mut Dynamic>,
        node: impl Into<ASTNode<'a>>,
    ) -> RawrResultOf<()> {
        if self.is_debugger_registered()
            && let Some(cmd) = self.dbg_reset_raw(global, caches, scope, this_ptr, node)?
        {
            global.debugger_mut().status = cmd;
        }

        Ok(())
    }
    /// Run the debugger callback if there is a debugging interface registered.
    ///
    /// Returns [`Some`] if the debugger needs to be reactivated at the end of the block, statement or
    /// function call.
    ///
    /// It is up to the [`Engine`] to reactivate the debugger.
    #[inline(always)]
    pub(crate) fn dbg_reset<'a>(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        this_ptr: Option<&mut Dynamic>,
        node: impl Into<ASTNode<'a>>,
    ) -> RawrResultOf<Option<DebuggerStatus>> {
        if self.is_debugger_registered() {
            self.dbg_reset_raw(global, caches, scope, this_ptr, node)
        } else {
            Ok(None)
        }
    }
    /// Run the debugger callback.
    ///
    /// Returns [`Some`] if the debugger needs to be reactivated at the end of the block, statement or
    /// function call.
    ///
    /// It is up to the [`Engine`] to reactivate the debugger.
    #[inline]
    pub(crate) fn dbg_reset_raw<'a>(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        this_ptr: Option<&mut Dynamic>,
        node: impl Into<ASTNode<'a>>,
    ) -> RawrResultOf<Option<DebuggerStatus>> {
        let node = node.into();

        // Skip transitive nodes
        match node {
            ASTNode::Expr(Expr::Stmt(..)) | ASTNode::Stmt(Stmt::Expr(..)) => return Ok(None),
            _ => (),
        }

        match global.debugger {
            Some(ref dbg) => {
                let event = match dbg.status {
                    DebuggerStatus::Init => Some(DebuggerEvent::Start),
                    DebuggerStatus::NEXT if node.is_stmt() => Some(DebuggerEvent::Step),
                    DebuggerStatus::INTO if node.is_expr() => Some(DebuggerEvent::Step),
                    DebuggerStatus::STEP => Some(DebuggerEvent::Step),
                    DebuggerStatus::Terminate => Some(DebuggerEvent::End),
                    _ => None,
                };

                let event = match event {
                    Some(e) => e,
                    None => match dbg.is_break_point(global.source(), node) {
                        Some(bp) => DebuggerEvent::BreakPoint(bp),
                        None if self.is_debugger_break(node.position()) => DebuggerEvent::Break,
                        None if self.is_debugger_woken() => DebuggerEvent::Wake,
                        None => return Ok(None),
                    },
                };

                self.dbg_raw(global, caches, scope, this_ptr, node, event)
            }
            None => Ok(None),
        }
    }
    /// Run the debugger callback unconditionally.
    ///
    /// Returns [`Some`] if the debugger needs to be reactivated at the end of the block, statement or
    /// function call.
    ///
    /// It is up to the [`Engine`] to reactivate the debugger.
    #[inline]
    pub(crate) fn dbg_raw(
        &self,
        global: &mut GlobalRuntimeState,
        caches: &mut Caches,
        scope: &mut Scope,
        this_ptr: Option<&mut Dynamic>,
        node: ASTNode,
        event: DebuggerEvent,
    ) -> Result<Option<DebuggerStatus>, Box<crate::EvalAltResult>> {
        match self.debugger_interface {
            Some(ref x) => {
                let orig_scope_len = scope.len();

                let src = global.source_raw().cloned();
                let context = EvalContext::new(self, global, caches, scope, this_ptr);
                let (.., ref on_debugger) = *x;

                let command = on_debugger(context, event, node, src.as_deref(), node.position());

                if orig_scope_len != scope.len() {
                    // The scope is changed, always search from now on
                    global.always_search_scope = true;
                }

                match command? {
                    DebuggerCommand::Continue => {
                        global.debugger_mut().status = DebuggerStatus::CONTINUE;
                        Ok(None)
                    }
                    DebuggerCommand::Next => {
                        global.debugger_mut().status = DebuggerStatus::CONTINUE;
                        Ok(Some(DebuggerStatus::NEXT))
                    }
                    DebuggerCommand::StepOver => {
                        global.debugger_mut().status = DebuggerStatus::CONTINUE;
                        Ok(Some(DebuggerStatus::STEP))
                    }
                    DebuggerCommand::StepInto => {
                        global.debugger_mut().status = DebuggerStatus::STEP;
                        Ok(None)
                    }
                    DebuggerCommand::FunctionExit => {
                        // Bump a level if it is a function call
                        let level = match node {
                            ASTNode::Expr(Expr::FnCall(..)) | ASTNode::Stmt(Stmt::FnCall(..)) => {
                                global.level + 1
                            }
                            ASTNode::Stmt(Stmt::Expr(e)) if matches!(**e, Expr::FnCall(..)) => {
                                global.level + 1
                            }
                            _ => global.level,
                        };
                        global.debugger_mut().status = DebuggerStatus::FunctionExit(level);
                        Ok(None)
                    }
                }
            }
            None => Ok(None),
        }
    }
}
