//! Opaque data owned by a resolved Rawr program.

mod authority;
pub mod compile;
pub mod construction;
mod error;
mod executable;
mod generation;
pub(crate) mod hosted;
pub(crate) mod model;
pub(crate) mod origin;
mod path;
pub mod schema;
pub(crate) mod signatures;
pub(crate) mod source;

pub use authority::Authority;
pub use error::{Aspect, Error};
#[cfg(feature = "rust-import")]
pub(crate) use executable::ContractEvidence;
pub(crate) use executable::{ApplicationEvidence, Evidence};
pub use executable::{Executable, Positions};
pub use generation::GenerationId;
pub(crate) use path::Path;
pub use source::Source;
