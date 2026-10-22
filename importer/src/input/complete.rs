//! Complete public Build-message evidence.

use super::build;

/// Public Cargo messages proven complete against the equal post-Build unit graph.
pub(crate) struct Complete {
    pub(super) build: build::Build,
}
