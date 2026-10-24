//! Historical record persistence under the caller's original input account.
//! A byte commitment is not a current-input or bundle-publication receipt.

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

use super::super::{observe, Budget, Error, Result};
use super::Record;
use crate::cargo::{self, resolution::layout};

/// Exact historical bytes. The enclosing node must separately authenticate the
/// bundle, replay all sections and validate current inputs before reuse.
pub(crate) struct Commitment {
    length: usize,
    digest: cargo::Digest,
}

impl Record<'_> {
    /// Write once inside the live Attempt. Failure leaves cleanup to that owner;
    /// it never replaces an existing record or grants publication authority.
    pub(crate) fn persist(
        &self,
        layout: &layout::Layout,
        budget: &mut Budget<'_>,
    ) -> Result<Commitment> {
        let result = (|| {
            budget.check()?;
            if !layout.budget().shares_with(budget.graph()) {
                return Err(Error::Encoding("resolution record graph account differs"));
            }
            budget.limited(
                "retained resolution record bytes",
                budget.limits.input_proof_bytes,
                self.encoded.len(),
            )?;
            for (resource, bytes) in [
                ("retained Cargo metadata bytes", self.metadata),
                ("retained Cargo unit bytes", self.units),
            ] {
                budget.limited(resource, budget.limits.json_bytes, bytes.len())?;
            }
            let digest = budget.digest(self.encoded)?;
            let path = layout.paths().record();
            admit_path(path, budget)?;
            // Reserve close before opening. Each write and sync is charged below.
            budget.paths(2)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)
                .map_err(|error| io("create retained resolution record", error))?;
            let mut remaining = self.encoded;
            while !remaining.is_empty() {
                budget.paths(1)?;
                match file.write(remaining) {
                    Ok(0) => {
                        return Err(io(
                            "write retained resolution record",
                            io::Error::from(io::ErrorKind::WriteZero),
                        ));
                    }
                    Ok(count) => remaining = &remaining[count..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(io("write retained resolution record", error)),
                }
            }
            budget.paths(1)?;
            file.sync_all()
                .map_err(|error| io("sync retained resolution record", error))?;
            Ok(Commitment {
                length: self.encoded.len(),
                digest,
            })
        })();
        result.map_err(|error| budget.failure(error))
    }
}

impl Commitment {
    pub(crate) fn encode(&self) -> [u8; 40] {
        let mut bytes = [0; 40];
        bytes[..8].copy_from_slice(&(self.length as u64).to_le_bytes());
        bytes[8..].copy_from_slice(self.digest.as_bytes());
        bytes
    }

    /// Decode the bundle's record commitment. The record extent is a store
    /// payload bound by the physical content ceiling, like the bundle seal;
    /// the `input_proof_bytes` accounting target is measured against it by
    /// `read`, on the account that spends the read.
    pub(crate) fn decode(bytes: &[u8; 40]) -> Result<Self> {
        let mut length = [0; 8];
        length.copy_from_slice(&bytes[..8]);
        let length = usize::try_from(u64::from_le_bytes(length)).map_err(|_| Error::Overflow)?;
        if length == 0 || length > crate::node::MAX_CONTENT_BYTES {
            return Err(Error::Encoding(
                "retained resolution record extent is invalid",
            ));
        }
        let mut digest = [0; 32];
        digest.copy_from_slice(&bytes[8..]);
        Ok(Self {
            length,
            digest: cargo::Digest::from_bytes(digest),
        })
    }

    /// Read from already bundle-validated geometry, spending this invocation's
    /// actual account. No repair, Cargo work, or fresh authority is produced.
    pub(crate) fn read(&self, paths: &layout::Paths, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
        let result = (|| {
            budget.check()?;
            budget.limited(
                "retained resolution record bytes",
                budget.limits.input_proof_bytes,
                self.length,
            )?;
            let path = paths.record();
            admit_path(path, budget)?;
            budget.paths(2)?;
            // NONBLOCK prevents a substituted FIFO from stalling before fstat.
            let mut file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
                .open(path)
                .map_err(|error| io("open retained resolution record", error))?;
            require_file(&file, self.length, budget)?;
            let mut bytes = budget.buffer(self.length)?;
            bytes.resize(self.length, 0);
            let mut remaining = bytes.as_mut_slice();
            while !remaining.is_empty() {
                budget.paths(1)?;
                match file.read(remaining) {
                    Ok(0) => return Err(Error::Encoding("retained resolution record shortened")),
                    Ok(count) => remaining = &mut remaining[count..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(io("read retained resolution record", error)),
                }
            }
            loop {
                budget.paths(1)?;
                match file.read(&mut [0]) {
                    Ok(0) => break,
                    Ok(_) => return Err(Error::Encoding("retained resolution record grew")),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(io("finish retained resolution record", error)),
                }
            }
            require_file(&file, self.length, budget)?;
            if budget.digest(&bytes)? != self.digest {
                return Err(Error::Encoding("retained resolution record digest changed"));
            }
            Ok(bytes)
        })();
        result.map_err(|error| budget.failure(error))
    }
}

fn require_file(file: &File, length: usize, budget: &mut Budget<'_>) -> Result<()> {
    budget.paths(1)?;
    let metadata = file
        .metadata()
        .map_err(|error| io("inspect retained resolution record", error))?;
    if !metadata.is_file()
        || crate::store::bundle::hard_linked(&metadata)
        || metadata.len() != length as u64
    {
        return Err(Error::Encoding(
            "retained resolution record file shape changed",
        ));
    }
    Ok(())
}

fn admit_path(path: &Path, budget: &mut Budget<'_>) -> Result<()> {
    let bytes = crate::store::path::input_allocation(path)?;
    if bytes != 0 {
        budget.storage(1, bytes)?;
    }
    Ok(())
}

fn io(operation: &'static str, source: io::Error) -> Error {
    observe::Error::Io { operation, source }.into()
}
