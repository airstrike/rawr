//! Concrete pinned visitor specializations over the common value adapters.

pub(super) mod dependency;
pub(super) mod hints;
pub(super) mod lints;
pub(super) mod manifest;
pub(super) mod package;
pub(super) mod platform;
pub(super) mod primitive;
pub(super) mod profile;
mod target;
mod trim;
pub(super) mod workspace;

pub(super) use target::Target;
pub(super) use trim::{Paths, Trim};
