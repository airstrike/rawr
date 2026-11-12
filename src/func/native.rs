//! Module defining interfaces to native-Rust functions.

use super::call::FnCallArgs;
use crate::ast::FnCallHashes;
use crate::eval::{Caches, GlobalRuntimeState};
use crate::plugin::PluginFunc;
use crate::tokenizer::{is_valid_function_name, Token, TokenizeState};
use crate::types::dynamic::Variant;
use crate::{
    calc_fn_hash, expose_under_internals, Dynamic, Engine, EvalContext, FnArgsVec, FuncArgs,
    Position, RawrResult, RawrResultOf, StaticVec, VarDefInfo, ERR,
};
use std::any::type_name;

/// Trait that maps to `Send + Sync` only under the `sync` feature.
#[cfg(feature = "sync")]
pub trait SendSync: Send + Sync {}
/// Trait that maps to `Send + Sync` only under the `sync` feature.
#[cfg(feature = "sync")]
impl<T: Send + Sync> SendSync for T {}

/// Trait that maps to `Send + Sync` only under the `sync` feature.
#[cfg(not(feature = "sync"))]
pub trait SendSync {}
/// Trait that maps to `Send + Sync` only under the `sync` feature.
#[cfg(not(feature = "sync"))]
impl<T> SendSync for T {}

/// Immutable reference-counted container.
#[cfg(not(feature = "sync"))]
pub use std::rc::Rc as Shared;
/// Immutable reference-counted container.
#[cfg(feature = "sync")]
pub use std::sync::Arc as Shared;

/// Synchronized shared object.
#[cfg(not(feature = "sync"))]
pub use std::cell::RefCell as Locked;

/// Read-only lock guard for synchronized shared object.
#[cfg(not(feature = "sync"))]
pub type LockGuard<'a, T> = std::cell::Ref<'a, T>;

/// Mutable lock guard for synchronized shared object.
#[cfg(not(feature = "sync"))]
pub type LockGuardMut<'a, T> = std::cell::RefMut<'a, T>;

/// Synchronized shared object.
#[cfg(feature = "sync")]
#[allow(dead_code)]
pub use std::sync::RwLock as Locked;

/// Read-only lock guard for synchronized shared object.
#[cfg(feature = "sync")]
#[allow(dead_code)]
pub type LockGuard<'a, T> = std::sync::RwLockReadGuard<'a, T>;

/// Mutable lock guard for synchronized shared object.
#[cfg(feature = "sync")]
#[allow(dead_code)]
pub type LockGuardMut<'a, T> = std::sync::RwLockWriteGuard<'a, T>;

/// Context of a native Rust function call.
#[derive(Debug)]
pub struct NativeCallContext<'a> {
    /// The current [`Engine`].
    engine: &'a Engine,
    /// Name of function called.
    fn_name: &'a str,
    /// Function source, if any.
    source: Option<&'a str>,
    /// The current [`GlobalRuntimeState`], if any.
    global: &'a GlobalRuntimeState,
    /// [Position] of the function call.
    pos: Position,
}

/// _(internals)_ Context of a native Rust function call, intended for persistence.
/// Exported under the `internals` feature only.
///
/// # WARNING - Volatile Type
///
/// This type is volatile and may change in the future.
#[deprecated = "This type is NOT deprecated, but it is considered volatile and may change in the future."]
#[cfg(feature = "internals")]
#[derive(Debug, Clone)]
pub struct NativeCallContextStore {
    /// Name of function called.
    pub fn_name: String,
    /// Function source, if any.
    pub source: Option<String>,
    /// The current [`GlobalRuntimeState`], if any.
    pub global: GlobalRuntimeState,
    /// [Position] of the function call.
    pub pos: Position,
}

#[cfg(feature = "internals")]
#[allow(deprecated)]
impl NativeCallContextStore {
    /// Create a [`NativeCallContext`] from a [`NativeCallContextStore`].
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[inline(always)]
    #[must_use]
    pub fn create_context<'a>(&'a self, engine: &'a Engine) -> NativeCallContext<'a> {
        NativeCallContext::from_stored_data(engine, self)
    }
}

impl<'a>
    From<(
        &'a Engine,
        &'a str,
        Option<&'a str>,
        &'a GlobalRuntimeState,
        Position,
    )> for NativeCallContext<'a>
{
    #[inline(always)]
    fn from(
        value: (
            &'a Engine,
            &'a str,
            Option<&'a str>,
            &'a GlobalRuntimeState,
            Position,
        ),
    ) -> Self {
        Self {
            engine: value.0,
            fn_name: value.1,
            source: value.2,
            global: value.3,
            pos: value.4,
        }
    }
}

impl<'a> NativeCallContext<'a> {
    /// _(internals)_ Create a new [`NativeCallContext`].
    /// Exported under the `internals` feature only.
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub const fn new_with_all_fields(
        engine: &'a Engine,
        fn_name: &'a str,
        source: Option<&'a str>,
        global: &'a GlobalRuntimeState,
        pos: Position,
    ) -> Self {
        Self {
            engine,
            fn_name,
            source,
            global,
            pos,
        }
    }

    /// _(internals)_ Create a [`NativeCallContext`] from a [`NativeCallContextStore`].
    /// Exported under the `internals` feature only.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[cfg(feature = "internals")]
    #[inline]
    #[must_use]
    #[allow(deprecated)]
    pub fn from_stored_data(engine: &'a Engine, context: &'a NativeCallContextStore) -> Self {
        Self {
            engine,
            fn_name: &context.fn_name,
            source: context.source.as_deref(),
            global: &context.global,
            pos: context.pos,
        }
    }
    /// _(internals)_ Store this [`NativeCallContext`] into a [`NativeCallContextStore`].
    /// Exported under the `internals` feature only.
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[cfg(feature = "internals")]
    #[inline]
    #[must_use]
    #[allow(deprecated)]
    pub fn store_data(&self) -> NativeCallContextStore {
        let mut global = self.global.clone();
        // The store can outlive the calls on its call stack.
        #[cfg(feature = "debugging")]
        if let Some(debugger) = global.debugger.as_mut() {
            debugger.forget_bindings();
        }
        NativeCallContextStore {
            fn_name: self.fn_name.to_string(),
            source: self.source.map(ToString::to_string),
            global,
            pos: self.pos,
        }
    }

    /// The current [`Engine`].
    #[inline(always)]
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        self.engine
    }

    pub(crate) fn host_runtime(&self) -> Option<&'a crate::host::runtime::Registry> {
        self.global.host_runtime.as_deref()
    }

    pub(crate) fn host_invocation(&self) -> Option<&'a crate::host::runtime::invocation::Proof> {
        self.global.host_invocation.as_ref()
    }
    /// Name of the function called.
    #[inline(always)]
    #[must_use]
    pub const fn fn_name(&self) -> &str {
        self.fn_name
    }
    /// Source of the function.
    #[inline(always)]
    #[must_use]
    pub const fn fn_source(&self) -> Option<&str> {
        self.source
    }
    /// Source of the caller.
    #[inline(always)]
    #[must_use]
    pub fn call_source(&self) -> Option<&str> {
        self.global.source.as_deref()
    }
    /// [Position] of the function call in the caller.
    #[inline(always)]
    #[must_use]
    pub const fn call_position(&self) -> Position {
        self.pos
    }
    /// Current nesting level of function calls.
    #[inline(always)]
    #[must_use]
    pub const fn call_level(&self) -> usize {
        self.global.level
    }
    /// Custom state kept in a [`Dynamic`].
    #[inline(always)]
    #[must_use]
    pub const fn tag(&self) -> Option<&Dynamic> {
        Some(&self.global.tag)
    }
    /// Get an iterator over the current set of modules imported via `import` statements in reverse order
    /// (i.e. modules imported last come first).
    #[inline]
    pub fn iter_imports(&self) -> impl Iterator<Item = (&str, &crate::Module)> {
        self.global.iter_imports()
    }
    /// Get an iterator over the namespaces containing definitions of all script-defined functions.
    #[inline]
    pub fn iter_namespaces(&self) -> impl Iterator<Item = &crate::Module> {
        self.global.lib.iter().map(<_>::as_ref)
    }
    /// _(internals)_ The current stack of namespaces containing definitions of all script-defined functions.
    /// Exported under the `internals` feature only.
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub fn namespaces(&self) -> &[crate::SharedModule] {
        &self.global.lib
    }
    /// _(internals)_ The current [`GlobalRuntimeState`], if any.
    /// Exported under the `internals` feature only.
    #[expose_under_internals]
    #[inline(always)]
    #[must_use]
    const fn global_runtime_state(&self) -> &GlobalRuntimeState {
        self.global
    }
    /// _(internals, debugging)_ Debugging interface.
    /// Exported under the `debugging` and `internals` features only.
    #[cfg(feature = "debugging")]
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub fn debugger(&self) -> Option<&crate::debugger::Debugger> {
        self.global.debugger.as_deref()
    }

    /// Call a function inside the call context with the provided arguments.
    #[inline]
    pub fn call_fn<T: Variant + Clone>(
        &self,
        fn_name: impl AsRef<str>,
        args: impl FuncArgs,
    ) -> RawrResultOf<T> {
        let mut arg_values = StaticVec::new();
        args.parse(&mut arg_values);

        let args = &mut arg_values.iter_mut().collect::<FnArgsVec<_>>();

        self._call_fn_raw(fn_name, args, false, false, false)
            .and_then(|result| {
                result.try_cast_result().map_err(|r| {
                    let result_type = self.engine().map_type_name(r.type_name());
                    let cast_type = match type_name::<T>() {
                        typ if typ.contains("::") => self.engine.map_type_name(typ),
                        typ => typ,
                    };
                    ERR::ErrorMismatchOutputType(
                        cast_type.into(),
                        result_type.into(),
                        self.call_position(),
                    )
                    .into()
                })
            })
    }
    /// Call a function as a _method_ call inside the call context with the provided arguments
    /// and a bound `this` pointer.
    #[inline]
    pub fn call_method<T: Variant + Clone>(
        &self,
        fn_name: impl AsRef<str>,
        this_ptr: &mut Dynamic,
        args: impl FuncArgs,
    ) -> RawrResultOf<T> {
        let mut arg_values = StaticVec::new();
        args.parse(&mut arg_values);

        let args = &mut arg_values.iter_mut().collect::<FnArgsVec<_>>();
        args.insert(0, this_ptr);

        self._call_fn_raw(fn_name, args, false, true, true)
            .and_then(|result| {
                result.try_cast_result().map_err(|r| {
                    let result_type = self.engine().map_type_name(r.type_name());
                    let cast_type = match type_name::<T>() {
                        typ if typ.contains("::") => self.engine.map_type_name(typ),
                        typ => typ,
                    };
                    ERR::ErrorMismatchOutputType(
                        cast_type.into(),
                        result_type.into(),
                        self.call_position(),
                    )
                    .into()
                })
            })
    }
    /// Call a registered native Rust function inside the call context with the provided arguments.
    ///
    /// This is often useful because Rust functions typically only want to cross-call other
    /// registered Rust functions and not have to worry about scripted functions hijacking the
    /// process unknowingly (or deliberately).
    #[inline]
    pub fn call_native_fn<T: Variant + Clone>(
        &self,
        fn_name: impl AsRef<str>,
        args: impl FuncArgs,
    ) -> RawrResultOf<T> {
        let mut arg_values = StaticVec::new();
        args.parse(&mut arg_values);

        let args = &mut arg_values.iter_mut().collect::<FnArgsVec<_>>();

        self._call_fn_raw(fn_name, args, true, false, false)
            .and_then(|result| {
                result.try_cast_result().map_err(|r| {
                    let result_type = self.engine().map_type_name(r.type_name());
                    let cast_type = match type_name::<T>() {
                        typ if typ.contains("::") => self.engine.map_type_name(typ),
                        typ => typ,
                    };
                    ERR::ErrorMismatchOutputType(
                        cast_type.into(),
                        result_type.into(),
                        self.call_position(),
                    )
                    .into()
                })
            })
    }
    /// Call a registered native Rust function as a _method_ call inside the call context with the provided arguments
    /// and a bound object.
    ///
    /// This is often useful because Rust functions typically only want to cross-call other
    /// registered Rust functions and not have to worry about scripted functions hijacking the
    /// process unknowingly (or deliberately).
    #[inline]
    pub fn call_native_method<T: Variant + Clone>(
        &self,
        fn_name: impl AsRef<str>,
        object: &mut Dynamic,
        args: impl FuncArgs,
    ) -> RawrResultOf<T> {
        let mut arg_values = StaticVec::new();
        args.parse(&mut arg_values);

        let args = &mut arg_values.iter_mut().collect::<FnArgsVec<_>>();
        args.insert(0, object);

        self._call_fn_raw(fn_name, args, true, true, true)
            .and_then(|result| {
                result.try_cast_result().map_err(|r| {
                    let result_type = self.engine().map_type_name(r.type_name());
                    let cast_type = match type_name::<T>() {
                        typ if typ.contains("::") => self.engine.map_type_name(typ),
                        typ => typ,
                    };
                    ERR::ErrorMismatchOutputType(
                        cast_type.into(),
                        result_type.into(),
                        self.call_position(),
                    )
                    .into()
                })
            })
    }
    /// Call a function (native Rust or scripted) inside the call context.
    ///
    /// If `is_method_call` is [`true`], the first argument is assumed to be the `this` pointer for
    /// a script-defined function (or the object of a method call).
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is very low level.
    ///
    /// # Arguments
    ///
    /// All arguments may be _consumed_, meaning that they may be replaced by `()`. This is to avoid
    /// unnecessarily cloning the arguments.
    ///
    /// **DO NOT** reuse the arguments after this call. If they are needed afterwards, clone them
    /// _before_ calling this function.
    ///
    /// If `is_ref_mut` is [`true`], the first argument is assumed to be passed by reference and is
    /// not consumed.
    #[inline(always)]
    pub fn call_fn_raw(
        &self,
        fn_name: impl AsRef<str>,
        is_ref_mut: bool,
        is_method_call: bool,
        args: &mut [&mut Dynamic],
    ) -> RawrResult {
        let name = fn_name.as_ref();
        let native_only = !is_valid_function_name(name);
        let native_only = native_only && !crate::parser::is_anonymous_fn(name);

        self._call_fn_raw(fn_name, args, native_only, is_ref_mut, is_method_call)
    }
    /// Call a script function by its compiler-private name, the first
    /// argument its receiver by reference: a crate type's own method a
    /// native comparison calls.
    pub(crate) fn call_script_method(
        &self,
        fn_name: &str,
        args: &mut [&mut Dynamic],
    ) -> RawrResult {
        self._call_fn_raw(fn_name, args, false, true, true)
    }
    /// Call a registered native Rust function inside the call context.
    ///
    /// This is often useful because Rust functions typically only want to cross-call other
    /// registered Rust functions and not have to worry about scripted functions hijacking the
    /// process unknowingly (or deliberately).
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is very low level.
    ///
    /// # Arguments
    ///
    /// All arguments may be _consumed_, meaning that they may be replaced by `()`. This is to avoid
    /// unnecessarily cloning the arguments.
    ///
    /// **DO NOT** reuse the arguments after this call. If they are needed afterwards, clone them
    /// _before_ calling this function.
    ///
    /// If `is_ref_mut` is [`true`], the first argument is assumed to be passed by reference and is
    /// not consumed.
    #[inline(always)]
    pub fn call_native_fn_raw(
        &self,
        fn_name: impl AsRef<str>,
        is_ref_mut: bool,
        args: &mut [&mut Dynamic],
    ) -> RawrResult {
        self._call_fn_raw(fn_name, args, true, is_ref_mut, false)
    }

    /// Call a function (native Rust or scripted) inside the call context.
    fn _call_fn_raw(
        &self,
        fn_name: impl AsRef<str>,
        args: &mut [&mut Dynamic],
        native_only: bool,
        is_ref_mut: bool,
        is_method_call: bool,
    ) -> RawrResult {
        let global = &mut self.global.clone();
        global.level += 1;

        let caches = &mut Caches::new();

        let fn_name = fn_name.as_ref();
        let op_token = Token::lookup_symbol_from_syntax(fn_name);
        let args_len = args.len();

        // The names that are only syntax (`__rawr_curry`) have no
        // registered implementation: dispatch would report them unknown.
        if let Some(result) =
            self.engine()
                .exec_syntactic_fn_call(fn_name, args, self.call_position())
        {
            return result;
        }

        if native_only {
            return self
                .engine()
                .exec_native_fn_call(
                    global,
                    caches,
                    fn_name,
                    op_token.as_ref(),
                    calc_fn_hash(None, fn_name, args_len),
                    args,
                    is_ref_mut,
                    false,
                    false,
                    self.call_position(),
                )
                .map(|(r, ..)| r);
        }

        // Native or script

        let hash = match is_method_call {
            true => FnCallHashes::from_script_and_native(
                calc_fn_hash(None, fn_name, args_len - 1),
                calc_fn_hash(None, fn_name, args_len),
            ),
            _ => FnCallHashes::from_hash(calc_fn_hash(None, fn_name, args_len)),
        };

        self.engine()
            .exec_fn_call(
                global,
                caches,
                None,
                fn_name,
                op_token.as_ref(),
                hash,
                args,
                is_ref_mut,
                is_method_call,
                self.call_position(),
            )
            .map(|(r, ..)| r)
    }
}

/// Return a mutable reference to the wrapped value of a [`Shared`] resource.
/// If the resource is shared (i.e. has other outstanding references), a cloned copy is used.
#[inline(always)]
#[must_use]
#[allow(dead_code)]
pub fn shared_make_mut<T: Clone>(value: &mut Shared<T>) -> &mut T {
    Shared::make_mut(value)
}

/// Return a mutable reference to the wrapped value of a [`Shared`] resource.
#[inline(always)]
#[must_use]
#[allow(dead_code)]
pub fn shared_get_mut<T: Clone>(value: &mut Shared<T>) -> Option<&mut T> {
    Shared::get_mut(value)
}

/// Consume a [`Shared`] resource if is unique (i.e. not shared), or clone it otherwise.
#[inline]
#[must_use]
#[allow(dead_code)]
pub fn shared_take_or_clone<T: Clone>(value: Shared<T>) -> T {
    shared_try_take(value).unwrap_or_else(|v| v.as_ref().clone())
}

/// Consume a [`Shared`] resource if is unique (i.e. not shared).
#[inline(always)]
#[allow(dead_code)]
pub fn shared_try_take<T>(value: Shared<T>) -> Result<T, Shared<T>> {
    Shared::try_unwrap(value)
}

/// Consume a [`Shared`] resource, assuming that it is unique (i.e. not shared).
///
/// # Panics
///
/// Panics if the resource is shared (i.e. has other outstanding references).
#[inline]
#[must_use]
#[allow(dead_code)]
pub fn shared_take<T>(value: Shared<T>) -> T {
    shared_try_take(value)
        .ok()
        .unwrap_or_else(|| panic!("`value` is shared (i.e. has outstanding references)"))
}

/// _(internals)_ Lock a [`Locked`] resource for immutable access, or
/// `None` at once while it is borrowed for write, as
/// [`RefCell::try_borrow`] does: a value one thread shares is borrowed for
/// write only by a borrow still held, and waiting for it never ends.
/// Exported under the `internals` feature only.
///
/// [`RefCell::try_borrow`]: std::cell::RefCell::try_borrow
#[inline(always)]
#[must_use]
#[allow(dead_code)]
pub fn locked_read<T>(value: &Locked<T>) -> Option<LockGuard<'_, T>> {
    #[cfg(not(feature = "sync"))]
    return value.try_borrow().ok();

    // A holder that panicked left the value as it was, as a `RefCell` does.
    #[cfg(feature = "sync")]
    match value.try_read() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// _(internals)_ Lock a [`Locked`] resource for mutable access, or `None`
/// at once while it is borrowed, as [`RefCell::try_borrow_mut`] does.
/// Exported under the `internals` feature only.
///
/// [`RefCell::try_borrow_mut`]: std::cell::RefCell::try_borrow_mut
#[inline(always)]
#[must_use]
#[allow(dead_code)]
pub fn locked_write<T>(value: &Locked<T>) -> Option<LockGuardMut<'_, T>> {
    #[cfg(not(feature = "sync"))]
    return value.try_borrow_mut().ok();

    #[cfg(feature = "sync")]
    match value.try_write() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

/// Lock a [`Locked`] resource that threads share for immutable access,
/// waiting while another thread writes it. Only for state that no holder
/// reaches again while it holds it, as a cell's borrow list: a conflict
/// there is another thread's, and ends when that thread's access does.
#[inline(always)]
pub(crate) fn waited_read<T>(value: &Locked<T>) -> LockGuard<'_, T> {
    #[cfg(not(feature = "sync"))]
    return value.borrow();

    #[cfg(feature = "sync")]
    value
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Lock a [`Locked`] resource that threads share for mutable access,
/// waiting while another thread holds it, as [`waited_read`] does.
#[inline(always)]
pub(crate) fn waited_write<T>(value: &Locked<T>) -> LockGuardMut<'_, T> {
    #[cfg(not(feature = "sync"))]
    return value.borrow_mut();

    #[cfg(feature = "sync")]
    value
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Read the value a shared [`Dynamic`][crate::Dynamic] holds, to answer
/// what it is. A borrow for write still held as it is read is Rust's
/// `RefCell` panic, as `RefCell::borrow`'s is: never an answer that the
/// value is of another type.
#[inline]
pub(crate) fn shared_read(value: &Locked<crate::Dynamic>) -> LockGuard<'_, crate::Dynamic> {
    locked_read(value).unwrap_or_else(|| panic!("already mutably borrowed: BorrowError"))
}

/// Write the value a shared [`Dynamic`][crate::Dynamic] holds, as a type
/// that is asked for: a borrow still held is Rust's `RefCell` panic, as
/// `RefCell::borrow_mut`'s is ([`shared_read`]).
#[inline]
pub(crate) fn shared_write(value: &Locked<crate::Dynamic>) -> LockGuardMut<'_, crate::Dynamic> {
    locked_write(value).unwrap_or_else(|| panic!("already borrowed: BorrowMutError"))
}

/// General Rust function trait object.
#[cfg(not(feature = "sync"))]
pub type FnAny = dyn Fn(Option<NativeCallContext>, &mut FnCallArgs) -> RawrResult;
/// General Rust function trait object.
#[cfg(feature = "sync")]
pub type FnAny = dyn Fn(Option<NativeCallContext>, &mut FnCallArgs) -> RawrResult + Send + Sync;

/// Built-in function trait object.
pub type FnBuiltin = (
    fn(Option<NativeCallContext>, &mut FnCallArgs) -> RawrResult,
    bool,
);

/// Function that gets an iterator from a type.
#[cfg(not(feature = "sync"))]
pub type FnIterator = dyn Fn(Dynamic) -> Box<dyn Iterator<Item = RawrResultOf<Dynamic>>>;
/// Function that gets an iterator from a type.
#[cfg(feature = "sync")]
pub type FnIterator =
    dyn Fn(Dynamic) -> Box<dyn Iterator<Item = RawrResultOf<Dynamic>>> + Send + Sync;

/// Plugin function trait object.
#[cfg(not(feature = "sync"))]
pub type FnPlugin = dyn PluginFunc;
/// Plugin function trait object.
#[cfg(feature = "sync")]
pub type FnPlugin = dyn PluginFunc + Send + Sync;

/// Callback function for progress reporting.
#[cfg(not(feature = "sync"))]
pub type OnProgressCallback = dyn Fn(u64) -> Option<Dynamic>;
/// Callback function for progress reporting.
#[cfg(feature = "sync")]
pub type OnProgressCallback = dyn Fn(u64) -> Option<Dynamic> + Send + Sync;

/// Callback function for `print!`, `println!`, `eprint!` and `eprintln!`.
#[cfg(not(feature = "sync"))]
pub type OnOutputCallback = dyn Fn(crate::Output, &str);
/// Callback function for `print!`, `println!`, `eprint!` and `eprintln!`.
#[cfg(feature = "sync")]
pub type OnOutputCallback = dyn Fn(crate::Output, &str) + Send + Sync;

/// _(internals)_ Callback function when a property accessed is not found in a [`Map`][crate::Map].
/// Exported under the `internals` feature only.
#[cfg(not(feature = "sync"))]
#[cfg(feature = "internals")]
pub type OnInvalidArrayIndexCallback = dyn for<'a> Fn(
    &'a mut crate::Array,
    crate::INT,
    EvalContext,
) -> RawrResultOf<crate::Target<'a>>;
/// Callback function when a property accessed is not found in a [`Map`][crate::Map].
/// Exported under the `internals` feature only.
#[cfg(feature = "sync")]
#[cfg(feature = "internals")]
pub type OnInvalidArrayIndexCallback = dyn for<'a> Fn(&'a mut crate::Array, crate::INT, EvalContext) -> RawrResultOf<crate::Target<'a>>
    + Send
    + Sync;

/// _(internals)_ Callback function when a property accessed is not found in a [`Map`][crate::Map].
/// Exported under the `internals` feature only.
#[cfg(not(feature = "sync"))]
#[cfg(feature = "internals")]
pub type OnMissingMapPropertyCallback =
    dyn for<'a> Fn(&'a mut crate::Map, &str, EvalContext) -> RawrResultOf<crate::eval::Target<'a>>;
/// Callback function when a property accessed is not found in a [`Map`][crate::Map].
/// Exported under the `internals` feature only.
#[cfg(feature = "sync")]
#[cfg(feature = "internals")]
pub type OnMissingMapPropertyCallback = dyn for<'a> Fn(&'a mut crate::Map, &str, EvalContext) -> RawrResultOf<crate::eval::Target<'a>>
    + Send
    + Sync;

/// _(internals)_ Callback function when a function call is not found.
/// Exported under the `internals` feature only.
///
/// Not triggered for qualified function calls (e.g. `module::function()`).
#[cfg(not(feature = "sync"))]
#[cfg(feature = "internals")]
pub type OnMissingFunctionCallback =
    dyn Fn(&str, &mut [&mut Dynamic], bool, EvalContext) -> RawrResultOf<Option<Dynamic>>;
/// _(internals)_ Callback function when a function call is not found.
/// Exported under the `internals` feature only.
///
/// Not triggered for qualified function calls (e.g. `module::function()`).
#[cfg(feature = "sync")]
#[cfg(feature = "internals")]
pub type OnMissingFunctionCallback = dyn Fn(&str, &mut [&mut Dynamic], bool, EvalContext) -> RawrResultOf<Option<Dynamic>>
    + Send
    + Sync;

/// Callback function for mapping tokens during parsing.
#[cfg(not(feature = "sync"))]
pub type OnParseTokenCallback = dyn Fn(Token, Position, &TokenizeState) -> Token;
/// Callback function for mapping tokens during parsing.
#[cfg(feature = "sync")]
pub type OnParseTokenCallback = dyn Fn(Token, Position, &TokenizeState) -> Token + Send + Sync;

/// Callback function for variable access.
#[cfg(not(feature = "sync"))]
pub type OnVarCallback = dyn Fn(&str, usize, EvalContext) -> RawrResultOf<Option<Dynamic>>;
/// Callback function for variable access.
#[cfg(feature = "sync")]
pub type OnVarCallback =
    dyn Fn(&str, usize, EvalContext) -> RawrResultOf<Option<Dynamic>> + Send + Sync;

/// Callback function for variable definition.
#[cfg(not(feature = "sync"))]
pub type OnDefVarCallback = dyn Fn(bool, VarDefInfo, EvalContext) -> RawrResultOf<bool>;
/// Callback function for variable definition.
#[cfg(feature = "sync")]
pub type OnDefVarCallback =
    dyn Fn(bool, VarDefInfo, EvalContext) -> RawrResultOf<bool> + Send + Sync;

#[cfg(test)]
mod tests {
    use crate::{Dynamic, INT};

    /// A shared value and a copy of it, which shares its cell.
    fn shared() -> (Dynamic, Dynamic) {
        let value = Dynamic::from(1 as INT).into_shared();
        let copy = value.clone();
        (value, copy)
    }

    /// A borrow still held is a conflict at once for a `Dynamic` read,
    /// which its caller meets as one (#1823).
    #[test]
    fn a_borrowed_shared_value_is_none_at_once_as_a_dynamic() {
        let (mut value, copy) = shared();
        let _held = value
            .write_lock::<Dynamic>()
            .expect("an unborrowed value is written");
        assert!(copy.read_lock::<Dynamic>().is_none());
    }

    /// A borrow still held is Rust's `RefCell` panic, never an answer that
    /// the value is of another type (#1823).
    #[test]
    #[should_panic(expected = "already mutably borrowed")]
    fn a_borrowed_shared_value_is_no_other_type() {
        let (mut value, copy) = shared();
        let _held = value
            .write_lock::<Dynamic>()
            .expect("an unborrowed value is written");
        let _ = copy.is_int();
    }
}
