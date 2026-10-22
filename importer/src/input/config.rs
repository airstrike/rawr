//! Cargo configuration state retained by a normal-build baseline.

use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) candidates: Box<[(PathBuf, Option<[u8; 32]>)]>,
    pub(crate) cargo_env_overrides: bool,
    pub(crate) provenance: [u8; 32],
    pub(crate) projection: crate::cargo::config::projection::Projection,
    pub(crate) _allocation: crate::cargo::RequestAllocation,
}
