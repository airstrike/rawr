//! Compiler-proven Rawr application roots.

mod bound;
pub mod callback;
mod candidate;
pub(crate) mod descriptor;
mod entry;
mod error;
mod instance;
mod launch;
pub mod startup;
mod type_ref;

pub use bound::{BoundLaunch, RuntimeError};
pub use candidate::Candidate;
pub use descriptor::{
    Boot, Callback, CheckedContract, Prelude, Descriptor, Fingerprint, SemanticFingerprint, Subscription,
    Task, Theme, Title, Update, View,
};
pub use entry::{Entry, Form};
pub use error::{Category, Error};
pub use instance::Instance;
pub use launch::Launch;
pub use startup::Startup;
pub(crate) use type_ref::RuntimeGraph;
pub use type_ref::Type;
