//! Main module defining the script evaluation [`Engine`].

use crate::api::default_limits::MAX_STRINGS_INTERNED;
use crate::api::options::LangOptions;
use crate::func::native::{
    locked_write, OnDebugCallback, OnDefVarCallback, OnOutputCallback, OnParseTokenCallback,
    OnPrintCallback, OnVarCallback,
};
use crate::packages::{Package, StandardPackage};
use crate::tokenizer::Token;
use crate::types::enums::EnumRegistration;
use crate::types::StringsInterner;
use crate::{Dynamic, Identifier, ImmutableString, Locked, SharedModule};
use std::{collections::BTreeSet, fmt, num::NonZeroU8};

pub type Precedence = NonZeroU8;

pub const KEYWORD_PRINT: &str = "print";
pub const KEYWORD_DEBUG: &str = "debug";
pub const KEYWORD_TYPE_OF: &str = "type_of";
pub const KEYWORD_EVAL: &str = "eval";
pub const KEYWORD_FN_PTR: &str = "Fn";
pub const KEYWORD_FN_PTR_CALL: &str = "call";
pub const KEYWORD_FN_PTR_CURRY: &str = "curry";
pub const KEYWORD_IS_SHARED: &str = "is_shared";
pub const KEYWORD_IS_DEF_VAR: &str = "is_def_var";
/// A move out of a local, which checked Rawr writes where Rust moves a
/// value that is not `Copy`: the value, with `()` left in the local.
pub const KEYWORD_MOVE: &str = "__rawr_move";
/// Rust's `drop(value)`: the value drops here, and the call is `()`, so a
/// statement of it keeps nothing.
pub const KEYWORD_DROP: &str = "__rawr_drop";
pub const KEYWORD_IS_DEF_FN: &str = "is_def_fn";
pub const KEYWORD_THIS: &str = "this";
pub const KEYWORD_GLOBAL: &str = "global";
pub const FN_GET: &str = "get$";
pub const FN_SET: &str = "set$";
pub const FN_IDX_GET: &str = "index$get$";
pub const FN_IDX_SET: &str = "index$set$";
pub const FN_ANONYMOUS: &str = "anon$";

/// Standard equality comparison operator.
///
/// Some standard functions (e.g. searching an [`Array`][crate::Array]) implicitly call this
/// function to compare two [`Dynamic`] values.
pub const OP_EQUALS: &str = Token::EqualsTo.literal_syntax();

/// Standard containment testing function.
///
/// The `in` operator is implemented as a call to this function.
pub const OP_CONTAINS: &str = "contains";

/// Standard not operator.
pub const OP_NOT: &str = Token::Bang.literal_syntax();

/// Separator for namespaces.
pub const NAMESPACE_SEPARATOR: &str = Token::DoubleColon.literal_syntax();

/// Rawr main scripting engine.
///
/// # Thread Safety
///
/// [`Engine`] is re-entrant.
///
/// Currently, [`Engine`] is neither [`Send`] nor [`Sync`].
/// Use the `sync` feature to make it [`Send`] `+` [`Sync`].
///
/// # Example
///
/// ```
/// # fn main() -> Result<(), Box<rawr::EvalAltResult>> {
/// use rawr::Engine;
///
/// let engine = Engine::new();
///
/// let result = engine.eval::<i64>("40 + 2")?;
///
/// println!("Answer: {result}");   // prints 42
/// # Ok(())
/// # }
/// ```
pub struct Engine {
    /// Lazily-issued compiler provenance identity.
    pub(crate) origin: crate::program::origin::Slot,
    /// A collection of all modules loaded into the global namespace of the Engine.
    pub(crate) global_modules: Vec<SharedModule>,
    /// A collection of all sub-modules directly loaded into the Engine.
    pub(crate) global_sub_modules: std::collections::BTreeMap<Identifier, SharedModule>,

    /// A module resolution service.
    pub(crate) module_resolver: Option<Box<dyn crate::ModuleResolver>>,

    /// Strings interner.
    pub(crate) interned_strings: Option<Locked<StringsInterner>>,

    /// A set of symbols to disable.
    pub(crate) disabled_symbols: BTreeSet<Identifier>,
    /// Registered Rust enum types available to `switch` patterns.
    pub(crate) registered_enums: std::collections::BTreeMap<Identifier, EnumRegistration>,
    /// Host type paths available while resolving Rawr item headers.
    pub(crate) host_registry: crate::host::Registry,
    /// Native implementations of the app's exported items, which every
    /// evaluation this engine starts can call.
    pub(crate) host_runtime: Option<crate::Shared<crate::host::runtime::Registry>>,
    /// Set by the first compile that calls an exported item, after which
    /// the exports can't change (D494 item 15).
    pub(crate) exports_frozen: core::sync::atomic::AtomicBool,
    /// Actual generated native metadata ownership, retained by every authority
    /// borrowing this compiler. It does not attest a live runner by itself.
    #[cfg(feature = "rust-import")]
    pub(crate) imported_native: Option<crate::package::import::runner::account::Native>,
    /// A map containing custom keywords and precedence to recognize.
    pub(crate) custom_keywords: std::collections::BTreeMap<Identifier, Option<Precedence>>,
    /// Custom syntax.
    pub(crate) custom_syntax:
        std::collections::BTreeMap<Identifier, Box<crate::api::custom_syntax::CustomSyntax>>,

    /// Callback closure for filtering variable definition.
    pub(crate) def_var_filter: Option<Box<OnDefVarCallback>>,
    /// Callback closure for resolving variable access.
    pub(crate) resolve_var: Option<Box<OnVarCallback>>,
    /// Callback closure to remap tokens during parsing.
    pub(crate) token_mapper: Option<Box<OnParseTokenCallback>>,

    /// Callback closure when a [`Array`][crate::Array] property accessed does not exist.
    #[cfg(feature = "internals")]
    pub(crate) invalid_array_index: Option<Box<crate::func::native::OnInvalidArrayIndexCallback>>,
    /// Callback closure when a [`Map`][crate::Map] property accessed does not exist.
    #[cfg(feature = "internals")]
    pub(crate) missing_map_property: Option<Box<crate::func::native::OnMissingMapPropertyCallback>>,
    /// Callback closure when a function call is not found.
    #[cfg(feature = "internals")]
    pub(crate) missing_function: Option<Box<crate::func::native::OnMissingFunctionCallback>>,

    /// Callback closure for implementing the `print` command.
    pub(crate) print: Option<Box<OnPrintCallback>>,
    /// Callback closure for implementing the `debug` command.
    pub(crate) debug: Option<Box<OnDebugCallback>>,
    /// Callback closure for `print!`, `println!`, `eprint!` and `eprintln!`.
    pub(crate) output: Option<Box<OnOutputCallback>>,
    /// Callback closure for progress reporting.
    pub(crate) progress: Option<Box<crate::func::native::OnProgressCallback>>,

    /// Language options.
    pub(crate) options: LangOptions,

    /// Default value for the custom state.
    pub(crate) def_tag: Dynamic,

    /// Script optimization level.
    pub(crate) optimization_level: crate::OptimizationLevel,

    /// Max limits.
    pub(crate) limits: crate::api::limits::Limits,

    /// Callback closure for debugging.
    #[cfg(feature = "debugging")]
    pub(crate) debugger_interface: Option<(
        Box<crate::eval::OnDebuggingInit>,
        Box<crate::eval::OnDebuggerCallback>,
    )>,
}

impl fmt::Debug for Engine {
    #[cold]
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut f = f.debug_struct("Engine");

        f.field("global_modules", &self.global_modules);

        f.field("global_sub_modules", &self.global_sub_modules);

        f.field("disabled_symbols", &self.disabled_symbols);

        f.field(
            "registered_enums",
            &self.registered_enums.keys().collect::<Vec<_>>(),
        )
        .field("host_registry", &self.host_registry);

        f.field("custom_keywords", &self.custom_keywords).field(
            "custom_syntax",
            &self
                .custom_syntax
                .keys()
                .map(crate::SmartString::as_str)
                .collect::<String>(),
        );

        f.field("def_var_filter", &self.def_var_filter.is_some())
            .field("resolve_var", &self.resolve_var.is_some())
            .field("token_mapper", &self.token_mapper.is_some());

        f.field("progress", &self.progress.is_some());

        f.field("options", &self.options)
            .field("default_tag", &self.def_tag);

        f.field("optimization_level", &self.optimization_level);

        f.field("limits", &self.limits);

        #[cfg(feature = "debugging")]
        f.field("debugger_interface", &self.debugger_interface.is_some());

        f.finish()
    }
}

impl Default for Engine {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

/// Make getter function
#[inline(always)]
#[must_use]
pub fn make_getter(id: &str) -> Identifier {
    let mut buf = Identifier::new_const();
    buf.push_str(FN_GET);
    buf.push_str(id);
    buf
}

/// Make setter function
#[inline(always)]
#[must_use]
pub fn make_setter(id: &str) -> Identifier {
    let mut buf = Identifier::new_const();
    buf.push_str(FN_SET);
    buf.push_str(id);
    buf
}

impl Engine {
    /// An empty raw [`Engine`].
    // Each moved const copy needs its own lazily issued provenance identity;
    // replacing this public const with a shared static or function changes that API.
    #[allow(clippy::declare_interior_mutable_const)]
    pub const RAW: Self = Self {
        origin: crate::program::origin::Slot::unissued(),
        global_modules: Vec::new(),

        global_sub_modules: std::collections::BTreeMap::new(),

        module_resolver: None,

        interned_strings: None,
        disabled_symbols: BTreeSet::new(),
        registered_enums: std::collections::BTreeMap::new(),
        host_registry: crate::host::Registry::new(),
        host_runtime: None,
        exports_frozen: core::sync::atomic::AtomicBool::new(false),
        #[cfg(feature = "rust-import")]
        imported_native: None,
        custom_keywords: std::collections::BTreeMap::new(),
        custom_syntax: std::collections::BTreeMap::new(),

        def_var_filter: None,
        resolve_var: None,
        token_mapper: None,

        #[cfg(feature = "internals")]
        invalid_array_index: None,
        #[cfg(feature = "internals")]
        missing_map_property: None,
        #[cfg(feature = "internals")]
        missing_function: None,

        print: None,
        debug: None,
        output: None,

        progress: None,

        options: LangOptions::new(),

        def_tag: Dynamic::UNIT,

        optimization_level: crate::OptimizationLevel::Simple,

        limits: crate::api::limits::Limits::new(),

        #[cfg(feature = "debugging")]
        debugger_interface: None,
    };

    /// Create a new [`Engine`].
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        // Create the new scripting Engine
        let mut engine = Self::new_raw();

        #[cfg(any(not(target_family = "wasm"), not(target_os = "unknown")))]
        {
            engine.module_resolver =
                Some(Box::new(crate::module::resolvers::FileModuleResolver::new()));
        }

        // Turn on the strings interner
        engine.set_max_strings_interned(MAX_STRINGS_INTERNED);

        // default print/debug implementations
        #[cfg(any(not(target_family = "wasm"), not(target_os = "unknown")))]
        {
            engine.print = Some(Box::new(|s| println!("{s}")));
            engine.debug = Some(Box::new(|s, source, pos| match (source, pos) {
                (Some(source), crate::Position::NONE) => println!("{source} | {s}"),
                (Some(source), pos) => println!("{source} @ {pos:?} | {s}"),
                (None, crate::Position::NONE) => println!("{s}"),
                (None, pos) => println!("{pos:?} | {s}"),
            }));
            engine.output = Some(Box::new(|output, text| match output {
                crate::Output::Stdout => print!("{text}"),
                crate::Output::Stderr => eprint!("{text}"),
            }));
        }

        // Register the standard package
        engine.register_global_module(StandardPackage::new().as_shared_module());

        // std's `path`, `ffi` and `io` types (`std_facade`).
        engine
            .host_registry
            .install(crate::std_facade::definition())
            .expect("a new engine has no other std facade");

        engine
    }

    /// Create a new [`Engine`] with minimal built-in functions.
    /// It returns a copy of [`Engine::RAW`].
    ///
    /// This is useful for creating a custom scripting engine with only the functions you need.
    ///
    /// Use [`register_global_module`][Engine::register_global_module] to add packages of functions.
    #[inline]
    #[must_use]
    pub const fn new_raw() -> Self {
        Self::RAW
    }

    pub(crate) fn origin(&self) -> crate::program::origin::Origin {
        self.origin.get()
    }

    /// Get an interned [string][ImmutableString].
    ///
    /// [`Engine`] keeps a cache of [`ImmutableString`] instances and tries to avoid new allocations
    /// and save memory when an existing instance is found.
    ///
    /// It is usually a good idea to intern strings if they are used frequently.
    #[inline]
    #[must_use]
    pub fn get_interned_string(
        &self,
        string: impl AsRef<str> + Into<ImmutableString>,
    ) -> ImmutableString {
        match self.interned_strings {
            Some(ref interner) => match locked_write(interner) {
                Some(mut cache) => cache.get(string),
                None => string.into(),
            },
            None => string.into(),
        }
    }
    /// Get an interned property getter, creating one if it is not yet interned.
    #[inline]
    #[must_use]
    pub(crate) fn get_interned_getter(
        &self,
        text: impl AsRef<str> + Into<ImmutableString>,
    ) -> ImmutableString {
        match self.interned_strings {
            Some(ref interner) => match locked_write(interner) {
                Some(mut cache) => {
                    cache.get_with_mapper(b'g', |s| make_getter(s.as_ref()).into(), text)
                }
                None => make_getter(text.as_ref()).into(),
            },
            None => make_getter(text.as_ref()).into(),
        }
    }

    /// Get an interned property setter, creating one if it is not yet interned.
    #[inline]
    #[must_use]
    pub(crate) fn get_interned_setter(
        &self,
        text: impl AsRef<str> + Into<ImmutableString>,
    ) -> ImmutableString {
        match self.interned_strings {
            Some(ref interner) => match locked_write(interner) {
                Some(mut cache) => {
                    cache.get_with_mapper(b's', |s| make_setter(s.as_ref()).into(), text)
                }
                None => make_setter(text.as_ref()).into(),
            },
            None => make_setter(text.as_ref()).into(),
        }
    }

    /// Get an empty [`ImmutableString`] which refers to a shared instance.
    #[inline(always)]
    #[must_use]
    pub fn const_empty_string(&self) -> ImmutableString {
        self.get_interned_string("")
    }

    /// Is there a debugger interface registered with this [`Engine`]?
    #[cfg(feature = "debugging")]
    #[inline(always)]
    #[must_use]
    pub(crate) const fn is_debugger_registered(&self) -> bool {
        self.debugger_interface.is_some()
    }
}
