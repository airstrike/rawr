//! Contained pinned Git selection and default upward discovery over supplied
//! facts. Native cwd association and repository opening remain separate.

mod config;
mod head;
mod loose;
mod opening;
mod path;
mod precompose;
mod predicate;
mod recognize;
mod repository;
mod trust;
mod upwards;
mod whitespace;

#[cfg(test)]
mod tests;
