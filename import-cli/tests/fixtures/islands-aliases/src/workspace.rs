//! Native: names the workspace crate's module and registry by `use`, and
//! declares an alias of a std type.
use model::extension::{self as extension_model, registry as extension_registry};

pub mod screen;

pub fn count(updates: &[extension_registry::Update]) -> usize {
    updates.len() + extension_model::registry::names().len()
}

/// An alias of a std type in the program's own module.
pub type Labels = Vec<String>;

pub fn labels() -> Labels {
    vec!["inbox".to_string(), "home".to_string()]
}
