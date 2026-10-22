//! Truthful authenticated-runner observation at the production spawn seam.

/// Exact facts available only after the generated runner authenticates.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Owned {
    pub runner_pid: u32,
    pub runner_uid: u32,
    pub build_request: [u8; 32],
    pub generated_manifest: [u8; 32],
    pub generation_key: [u8; 32],
    pub epoch: u64,
    pub artifact: [u8; 32],
    pub executable_path: Box<[u8]>,
    pub executable_digest: [u8; 32],
    pub executable_bytes: u64,
    pub spawned_ns: Option<u64>,
}

impl Owned {
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(super) fn valid(&self) -> bool {
        self.runner_pid != 0
            && self.build_request != [0; 32]
            && self.generated_manifest != [0; 32]
            && self.generation_key != [0; 32]
            && self.epoch != 0
            && self.artifact != [0; 32]
            && !self.executable_path.is_empty()
            && self.executable_digest != [0; 32]
            && self.executable_bytes != 0
            && self.spawned_ns.is_some()
    }
}
