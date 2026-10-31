//! # rawr
//!
//! rawr runs typed Rust as a scripting language. A Rust program embeds it
//! with an [`Engine`], which checks a script as Rust, runs it and calls its
//! functions.
//!
//! # A Quick Example
//!
//! ## Contents of `my_script.rs`
//!
//! ```rust,ignore
//! /// Brute force factorial function
//! fn factorial(x: i64) -> i64 {
//!     if x == 1 { return 1; }
//!     x * factorial(x - 1)
//! }
//!
//! // Calling an external function 'compute'
//! compute(factorial(10))
//! ```
//!
//! ## The Rust part
//!
//! ```no_run
//! use rawr::{Engine, EvalAltResult};
//!
//! fn main() -> Result<(), Box<EvalAltResult>>
//! {
//!     // Define external function
//!     fn compute_something(x: i64) -> bool {
//!         (x % 40) == 0
//!     }
//!
//!     // Create scripting engine
//!     let mut engine = Engine::new();
//!
//!     // Register external function as 'compute'
//!     engine.register_fn("compute", compute_something);
//!
//! #   #[cfg(not(target_family = "wasm"))]
//! #
//!     // Evaluate the script, expecting a 'bool' result
//!     let result: bool = engine.eval_file("my_script.rs".into())?;
//!
//!     assert_eq!(result, true);
//!
//!     Ok(())
//! }
//! ```
//!
//! # Features
//!
#![cfg_attr(feature = "document-features", doc = document_features::document_features!(
    feature_label = "<span id=\"feature-{feature}\">**`{feature}`**</span>"
))]
//!
//! # On-Line Documentation
//!
//! Project documentation lives in the
//! [Rawr repository](https://github.com/airstrike/rawr).

//
// Clippy lints.
//
#![deny(missing_docs)]
// #![warn(clippy::all)]
// #![warn(clippy::pedantic)]
// #![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::used_underscore_binding)]
#![allow(clippy::inline_always)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::box_collection)]
#![allow(clippy::upper_case_acronyms)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::unnecessary_box_returns)]
// Rawr's public structured error enums intentionally remain unboxed in this
// release; boxing them would change the established API and error ABI.
#![allow(clippy::result_large_err)]
// The workspace's upstream macro stacks currently require both syn 2 and syn 3.
#![allow(clippy::multiple_crate_versions)]
// The lints below are turned off to reduce signal/noise ratio
#![allow(clippy::cognitive_complexity)] // Hey, this is a scripting engine with a compiler...
#![allow(clippy::too_many_lines)] // Same...
#![allow(clippy::too_many_arguments)] // Same...
#![allow(clippy::wildcard_imports)] // Wildcard imports are used to import the plugins prelude
#![allow(clippy::enum_glob_use)] // Sometimes useful to import all `Tokens` etc.
#![allow(clippy::no_effect_underscore_binding)] // Underscored variables may be used by code within feature guards
#![allow(clippy::semicolon_if_nothing_returned)] // One-liner `match` cases are sometimes formatted as multi-line blocks

// The code emitted by codegen relies on this
extern crate self as rawr;

/// Version of the Rawr language runtime linked into this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Internal modules
#[macro_use]
mod reify;
#[macro_use]
mod defer;

mod api;
/// Compiler-proven Rawr application roots.
pub mod application;
mod ast;
/// Compile-time capabilities proven for Rawr values and callables.
pub mod capability;
pub mod config;
#[cfg(feature = "sync")]
pub mod crossing;
/// Canonical structured diagnostics shared by compilers, hosts, and tools.
pub mod diagnostic;
mod engine;
mod eval;
mod func;
/// Async code: what async bodies return, how they resume, and how a
/// runtime polls them.
pub mod future;
#[cfg(feature = "grain")]
pub mod grain;
/// Host type metadata visible during Rawr resolution.
pub mod host;
/// Source-level Rawr items.
#[cfg(feature = "rust-import")]
#[doc(hidden)]
pub mod island;
pub mod item;
pub mod layer;
/// Resolved inherent method metadata.
pub mod method;
mod module;
mod optimizer;
/// Rawr's `Option<T>` at runtime, and its round trip to Rust's `Option<T>`.
pub mod option;
/// Static Rawr source and local-package graphs.
pub mod package;
pub mod packages;
/// Parsed Rawr compilation units.
pub mod parse;
mod parser;
/// Typed native places and ownership-preserving sequence operations.
pub mod place;
/// Compiler-validated local preview metadata.
pub mod preview;
/// Opaque data retained by resolved programs.
pub mod program;
/// Canonical Rawr source and project discovery.
pub mod project;
/// Receiver modes proven for inherent methods.
pub mod receiver;
/// Immutable candidates for script-native program replacement.
pub mod reload;
/// Rawr item-name and header-type resolution.
pub mod resolve;
/// Rawr's `Result<T, E>` at runtime, and its round trip to Rust's `Result<T, E>`.
pub mod result;
/// Scoped loans: a borrow native code lends script code for one call.
pub mod scope;
#[cfg(feature = "serde")]
pub mod serde;
/// Canonical callable signature fingerprints.
pub mod signature;
/// Source text retained by parsed compilation units.
pub mod source;
mod std_facade;
mod tests;
/// Host-requested timing observations.
pub mod timing;
mod tokenizer;
/// Generation-local nominal type definitions.
pub mod type_def;
mod types;
mod value;
/// Compiler-proven scripted-widget lifecycle protocols.
pub mod widget;

/// Error encountered when parsing a script.
type PERR = ParseErrorType;
/// Evaluation result.
type ERR = EvalAltResult;
/// General evaluation error for Rawr scripts.
type RawrError = Box<ERR>;
/// Generic [`Result`] type for Rawr functions.
type RawrResultOf<T> = Result<T, RawrError>;
/// General [`Result`] type for Rawr functions returning [`Dynamic`] values.
type RawrResult = RawrResultOf<Dynamic>;

/// The system integer type. It is defined as [`i64`].
pub type INT = i64;

/// The unsigned system base integer type. It is defined as [`u64`].
#[allow(non_camel_case_types)]
type UNSIGNED_INT = u64;

/// Number of bits in [`INT`].
const INT_BITS: usize = std::mem::size_of::<INT>() * 8;

/// Number of bytes that make up an [`INT`].
const INT_BYTES: usize = std::mem::size_of::<INT>();

/// The system floating-point type. It is defined as [`f64`].
pub type FLOAT = f64;

/// Number of bytes that make up a [`FLOAT`].
const FLOAT_BYTES: usize = std::mem::size_of::<FLOAT>();

/// An exclusive integer range.
type ExclusiveRange = std::ops::Range<INT>;

/// An inclusive integer range.
type InclusiveRange = std::ops::RangeInclusive<INT>;

use once_cell::sync::OnceCell;

pub use api::build_type::{CustomType, TypeBuilder};
pub use api::custom_syntax::Expression;
pub use api::events::Output;
#[cfg(any(not(target_family = "wasm"), not(target_os = "unknown")))]
pub use api::files::{eval_file, run_file};
pub use api::{eval::eval, run::run};
pub use ast::{FnAccess, AST};
use defer::Deferred;
pub use engine::{Engine, OP_CONTAINS, OP_EQUALS};
pub use eval::{EvalContext, EvalContextFrameGuard};
use func::calc_typed_method_hash;
use func::{calc_fn_hash, calc_fn_hash_full, calc_var_hash};
pub use func::{plugin, FuncArgs, NativeCallContext, RawrNativeFunc};
pub use host::export::{Export, Exports};
pub use module::{FnNamespace, FuncRegistration, Module};
pub use packages::string_basic::{FUNC_TO_DEBUG, FUNC_TO_STRING};
pub use rawr_codegen::*;
pub use types::Instant;
pub use types::{
    Dynamic, EvalAltResult, FnPtr, ImmutableString, LexError, ParseError, ParseErrorType, Position,
    Scope, Span, VarDefInfo,
};
pub use value::Value;

/// _(debugging)_ Module containing types for debugging.
/// Exported under the `debugging` feature only.
#[cfg(feature = "debugging")]
pub mod debugger {
    pub use super::eval::CallStackFrame;
    pub use super::eval::{BreakPoint, Debugger, DebuggerCommand, DebuggerEvent};
}

/// _(internals)_ An identifier in Rawr.
/// Exported under the `internals` feature only.
///
/// Identifiers are assumed to be all-ASCII and short with few exceptions.
///
/// [`SmartString`](https://crates.io/crates/smartstring) is used as the underlying storage type
/// because most identifiers can be stored inline.
#[expose_under_internals]
type Identifier = SmartString;

/// Alias to [`Rc`][std::rc::Rc] or [`Arc`][std::sync::Arc] depending on the `sync` feature flag.
pub use func::Shared;

/// Alias to [`RefCell`][std::cell::RefCell] or [`RwLock`][std::sync::RwLock] depending on the `sync` feature flag.
pub use func::Locked;

/// A shared [`Module`].
type SharedModule = Shared<Module>;

pub use api::script::{Func, Parameters, Script};

pub use ast::ScriptFnMetadata;

pub use api::call_fn::CallFnOptions;

/// Variable-sized array of [`Dynamic`] values.
pub type Array = Vec<Dynamic>;

/// Variable-sized array of [`u8`] values (byte array).
pub type Blob = Vec<u8>;

/// A dictionary of [`Dynamic`] values with string keys.
///
/// [`SmartString`](https://crates.io/crates/smartstring) is used as the key type because most
/// property names are ASCII and short, fewer than 23 characters, so they can be stored inline.
pub type Map = std::collections::BTreeMap<Identifier, Dynamic>;

pub use api::json::format_map_as_json;

pub use module::ModuleResolver;

/// Module containing all built-in _module resolvers_ available to Rawr.
pub use module::resolvers as module_resolvers;

pub use optimizer::OptimizationLevel;

// Expose internal data structures.

#[cfg(feature = "internals")]
pub use types::dynamic::{AccessMode, DynamicReadLock, DynamicWriteLock, Variant};

#[cfg(feature = "internals")]
pub use module::{FuncInfo, FuncMetadata};

#[cfg(feature = "internals")]
pub use types::FloatWrapper;

#[cfg(feature = "internals")]
pub use types::{BloomFilterU64, CustomTypeInfo, StringsInterner};

#[cfg(feature = "internals")]
pub use tokenizer::{
    get_next_token, is_valid_function_name, is_valid_identifier, parse_raw_string_literal,
    parse_string_literal, InputStream, MultiInputsStream, Token, TokenIterator, TokenizeState,
    TokenizerControl, TokenizerControlBlock,
};

#[cfg(feature = "internals")]
pub use parser::ParseState;

#[cfg(feature = "internals")]
pub use api::default_limits;

#[cfg(feature = "internals")]
pub use ast::{
    ASTFlags, ASTNode, BinaryExpr, EncapsulatedEnviron, EnumCase, Expr, FlowControl, FnCallExpr,
    FnCallHashes, Ident, OpAssignment, RangeCase, ScriptFuncDef, Stmt, StmtBlock,
    SwitchCasesCollection,
};

#[cfg(feature = "internals")]
pub use ast::CustomExpr;

#[cfg(feature = "internals")]
pub use ast::Namespace;

#[cfg(feature = "internals")]
pub use eval::{Caches, FnResolutionCache, FnResolutionCacheEntry, GlobalRuntimeState, Target};

#[cfg(feature = "internals")]
#[allow(deprecated)]
pub use func::{locked_read, locked_write, NativeCallContextStore, RawrFunc};

/// Number of items to keep inline for [`StaticVec`].
const STATIC_VEC_INLINE_SIZE: usize = 3;

/// _(internals)_ Alias to [`smallvec::SmallVec<[T; 3]>`](https://crates.io/crates/smallvec),
/// which is a [`Vec`] backed by a small, inline, fixed-size array when there are ≤ 3 items stored.
/// Exported under the `internals` feature only.
///
/// # History
///
/// And Saint Attila raised the `SmallVec` up on high, saying, "O Lord, bless this Thy `SmallVec`
/// that, with it, Thou mayest blow Thine allocation costs to tiny bits in Thy mercy."
///
/// And the Lord did grin, and the people did feast upon the lambs and sloths and carp and anchovies
/// and orangutans and breakfast cereals and fruit bats and large chu...
///
/// And the Lord spake, saying, "First shalt thou depend on the [`smallvec`](https://crates.io/crates/smallvec) crate.
/// Then, shalt thou keep three inline. No more. No less. Three shalt be the number thou shalt keep inline,
/// and the number to keep inline shalt be three. Four shalt thou not keep inline, nor either keep inline
/// thou two, excepting that thou then proceed to three. Five is right out. Once the number three,
/// being the third number, be reached, then, lobbest thou thy `SmallVec` towards thy heap, who,
/// being slow and cache-naughty in My sight, shall snuff it."
///
/// # Why Three
///
/// `StaticVec` is used frequently to keep small lists of items in inline (non-heap) storage in
/// order to improve cache friendliness and reduce indirections.
///
/// The number 3, other than being the holy number, is carefully chosen for a balance between
/// storage space and reduce allocations. That is because most function calls (and most functions,
/// for that matter) contain fewer than 4 arguments, the exception being closures that capture a
/// large number of external variables.
///
/// In addition, most script blocks either contain many statements, or just one or two lines;
/// most scripts load fewer than 4 external modules; most module paths contain fewer than 4 levels
/// (e.g. `std::collections::map::HashMap` is 4 levels and it is just about as long as they get).
#[expose_under_internals]
type StaticVec<T> = smallvec::SmallVec<[T; STATIC_VEC_INLINE_SIZE]>;

/// _(internals)_ A smaller [`Vec`] alternative. Exported under the `internals` feature only.
/// Exported under the `internals` feature only.
///
/// The standard [`Vec`] type uses three machine words (i.e. 24 bytes on 64-bit).
///
/// [`ThinVec`](https://crates.io/crates/thin-vec) only uses one machine word, storing other
/// information inline together with the data.
///
/// This is primarily used in places where a few bytes affect the size of the type
/// -- e.g. in `enum`'s.
#[expose_under_internals]
type ThinVec<T> = thin_vec::ThinVec<T>;

/// Number of items to keep inline for [`FnArgsVec`].
const FN_ARGS_VEC_INLINE_SIZE: usize = 5;

/// _(internals)_ Inline arguments storage for function calls.
/// Exported under the `internals` feature only.
///
/// # Notes
///
/// Since most usage of this is during a function call to gather up arguments, this is mostly
/// allocated on the stack, so we can tolerate a larger number of values stored inline.
///
/// Most functions have few parameters, but closures with a lot of captured variables can
/// potentially have many.  Having a larger inline storage for arguments reduces allocations in
/// scripts with heavy closure usage.
#[expose_under_internals]
type FnArgsVec<T> = smallvec::SmallVec<[T; FN_ARGS_VEC_INLINE_SIZE]>;

type SmartString = smartstring::SmartString<smartstring::LazyCompact>;

// Compiler guards against mutually-exclusive feature flags

#[cfg(not(target_family = "wasm"))]
#[cfg(feature = "wasm-bindgen")]
compile_error!("`wasm-bindgen` cannot be used for non-WASM target");
