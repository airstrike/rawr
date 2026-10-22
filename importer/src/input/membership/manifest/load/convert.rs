//! Pinned post-normalization conversion stages; not complete Loaded authority.
pub(super) mod build;
pub(super) mod conflicts;
pub(super) mod contents;
pub(super) mod dependencies;
pub(super) mod dependency;
pub(super) mod edition;
pub(super) mod embedded;
pub(super) mod finish;
pub(super) mod kinds;
pub(super) mod lints;
pub(super) mod metadata;
pub(super) mod package;
pub(super) mod patch;
pub(super) mod platform;
pub(super) mod preamble;
pub(super) mod profiles;
pub(super) mod publish;
pub(super) mod real;
pub(super) mod replace;
pub(super) mod resolver;
pub(super) mod run;
pub(super) mod source;
pub(super) mod summary;
pub(super) mod target;
pub(super) mod url;
pub(super) mod value;
pub(super) mod workspace;

/// Initial real conversion results, before to_targets and manifest construction.
pub(super) struct Header {
    pub(super) edition: edition::Edition,
    pub(super) resolver: Option<resolver::Behavior>,
}

/// Consecutive ordinary then build targets; later manifest checks still follow.
pub(super) struct Targets<'a, 'd> {
    pub(super) ordinary: super::super::storage::Buffer<target::Target<'a, 'd>>,
    pub(super) builds: super::super::storage::Buffer<build::Target>,
}
