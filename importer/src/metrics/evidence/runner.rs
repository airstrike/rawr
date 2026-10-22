//! Sole authenticated runner-ownership receipt.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{process, Challenge, Error};

const RECEIPT_SCHEMA: &str = "rawr.phase9.evidence.runner-owned/v3";
const PATH_BYTES: usize = 16 * 1024;

/// One receipt emitted only after the real generated runner authenticates.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Receipt {
    schema: Box<str>,
    pub launch_id: [u8; 32],
    pub cli: process::Identity,
    pub runner: process::Identity,
    pub build_request: [u8; 32],
    pub generated_manifest: [u8; 32],
    pub generation_key: [u8; 32],
    pub epoch: u64,
    pub artifact: [u8; 32],
    pub executable_path_digest: [u8; 32],
    pub executable_digest: [u8; 32],
    pub executable_bytes: u64,
    pub spawned_ns: u64,
    pub title: super::title::Identity,
    pub title_digest: [u8; 32],
}

impl Receipt {
    pub(super) fn transform(
        challenge: &Challenge,
        cli: process::Identity,
        owned: &crate::metrics::runner::Owned,
    ) -> Result<Self, Error> {
        if owned.runner_pid == 0
            || owned.runner_pid == cli.pid
            || owned.runner_uid != cli.uid
            || owned.epoch == 0
            || owned.executable_digest == [0; 32]
            || owned.executable_bytes == 0
            || owned.executable_path.is_empty()
            || owned.executable_path.len() > PATH_BYTES
        {
            return Err(Error::Binding("runner receipt"));
        }
        let spawned_ns = owned.spawned_ns.ok_or(Error::Binding("runner clock"))?;
        if spawned_ns < challenge.t0_ns {
            return Err(Error::Binding("runner clock"));
        }
        let mut path = Sha256::new();
        path.update(b"rawr.phase9.runner-executable-path/v1");
        path.update((owned.executable_path.len() as u64).to_le_bytes());
        path.update(&owned.executable_path);
        Ok(Self {
            schema: RECEIPT_SCHEMA.into(),
            launch_id: challenge.launch_id,
            cli,
            runner: process::Identity::transform(owned.runner_pid, owned.runner_uid),
            build_request: owned.build_request,
            generated_manifest: owned.generated_manifest,
            generation_key: owned.generation_key,
            epoch: owned.epoch,
            artifact: owned.artifact,
            executable_path_digest: path.finalize().into(),
            executable_digest: owned.executable_digest,
            executable_bytes: owned.executable_bytes,
            spawned_ns,
            title: challenge.title.clone(),
            title_digest: challenge.title_digest,
        })
    }

    pub(super) fn validate(
        &self,
        challenge: &Challenge,
        child: process::Identity,
    ) -> Result<(), Error> {
        if self.schema.as_ref() != RECEIPT_SCHEMA
            || self.launch_id != challenge.launch_id
            || self.cli != child
            || self.runner.pid == 0
            || self.runner.pid == child.pid
            || self.runner.uid != child.uid
            || self.build_request == [0; 32]
            || self.generated_manifest == [0; 32]
            || self.generation_key == [0; 32]
            || self.artifact == [0; 32]
            || self.executable_digest == [0; 32]
            || self.executable_bytes == 0
            || self.epoch == 0
            || self.spawned_ns < challenge.t0_ns
            || self.title != challenge.title
            || self.title_digest != challenge.title_digest
        {
            return Err(Error::Binding("runner receipt"));
        }
        Ok(())
    }
}
