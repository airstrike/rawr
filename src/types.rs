//! Module defining Rawr data types.

pub mod bloom_filter;
pub mod custom_types;
pub mod dynamic;
pub(crate) mod enums;
pub mod error;
pub mod float;
pub mod fn_ptr;
pub mod immutable_string;
pub mod interner;
#[allow(dead_code)]
pub(crate) mod nominal;
pub mod parse_error;
pub mod position;
pub mod scope;
pub mod var_def;
pub mod variant;

pub use bloom_filter::BloomFilterU64;
pub use custom_types::{CustomTypeInfo, CustomTypesCollection};
pub use dynamic::Dynamic;
pub use dynamic::Instant;
pub use error::EvalAltResult;
pub use float::FloatWrapper;
pub use fn_ptr::FnPtr;
pub use immutable_string::ImmutableString;
pub use interner::StringsInterner;
pub use parse_error::{LexError, ParseError, ParseErrorType, Rendered};
pub use var_def::VarDefInfo;

pub use position::{Position, Span};

pub use scope::Scope;
pub use variant::Variant;
