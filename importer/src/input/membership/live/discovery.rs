//! Closed acquisition projection; never current-source authority.

use std::ffi::CStr;

use super::Stat;
use crate::input::membership::{observed, Budget, Error, Result};
use observed::discovery::{
    After, Attempt, Diagnostic, Exists, Filename, Identity, NativeError, Transcript,
};

#[cfg(test)]
mod tests;

/// The private native seam cannot read bodies, render warnings or mint records.
pub(super) trait System {
    fn open(&mut self, path: &CStr) -> std::result::Result<i32, i32>;
    fn fstat(&mut self, descriptor: i32) -> Result<Stat>;
    fn stat(&mut self, path: &CStr) -> Result<Stat>;
    fn close(&mut self, descriptor: i32);

    /// Test-visible logical boundary, not a renderer or warning-success fact.
    fn diagnostic(&mut self, _intent: Diagnostic) {}
}

#[derive(Clone, Copy)]
enum Slot {
    Legacy,
    Second,
}

/// Each fixed slot owns a descriptor only after its open succeeds.
/// One owner permits overlap without lending two mutable references to System.
struct Handles<'system, S: System> {
    legacy: Option<i32>,
    second: Option<i32>,
    system: &'system mut S,
}

impl<S: System> Handles<'_, S> {
    fn slot(&mut self, slot: Slot) -> &mut Option<i32> {
        match slot {
            Slot::Legacy => &mut self.legacy,
            Slot::Second => &mut self.second,
        }
    }

    fn close(&mut self, slot: Slot) {
        if let Some(descriptor) = self.slot(slot).take() {
            // The cleanup debit precedes its open. Never retry a close error.
            self.system.close(descriptor);
        }
    }

    fn acquire(&mut self, slot: Slot, path: &[u8], budget: &mut Budget<'_>) -> Result<Attempt> {
        let descriptor = {
            let native = terminated(path, budget)?;
            let native = native_path(&native)?;
            budget.graph.paths(1)?; // One potential close, reserved even if open fails.
            loop {
                budget.graph.paths(1)?;
                match self.system.open(native) {
                    Err(code) if code == nix::libc::EINTR => continue,
                    Err(code) => return Ok(Attempt::OpenFailed(native_error(code)?)),
                    Ok(descriptor) if descriptor >= 0 => break descriptor,
                    Ok(_) => {
                        return Err(Error::Encoding(
                            "discovery open returned a negative descriptor",
                        ))
                    }
                }
            }
        };
        *self.slot(slot) = Some(descriptor);
        budget.graph.paths(1)?;
        match self.system.fstat(descriptor) {
            Ok(Stat::Metadata(metadata)) => Ok(Attempt::Acquired(Identity {
                device: metadata.device(),
                inode: metadata.inode(),
            })),
            Ok(Stat::Error(code)) => {
                // same_file::from_file drops its File before returning failure.
                self.close(slot);
                native_error(code)?;
                Ok(Attempt::StatFailed(code))
            }
            Err(error) => {
                self.close(slot);
                Err(error)
            }
        }
    }
}

impl<S: System> Drop for Handles<'_, S> {
    fn drop(&mut self) {
        // The second if-let scope ends before the outer successful legacy scope.
        self.close(Slot::Second);
        self.close(Slot::Legacy);
    }
}

pub(super) fn acquire(
    directory: &[u8],
    filename: Filename,
    warn: bool,
    budget: &mut Budget<'_>,
    system: &mut impl System,
) -> Result<observed::Record> {
    let directory = observed::Path::from_bytes(directory, budget)?;
    // Both Path::join spellings exist before any acquisition in pinned Cargo.
    let legacy_path = joined(directory.bytes(), filename.legacy().as_bytes(), budget)?;
    let toml_path = joined(directory.bytes(), filename.toml().as_bytes(), budget)?;
    let mut handles = Handles {
        legacy: None,
        second: None,
        system,
    };
    let legacy = handles.acquire(Slot::Legacy, &legacy_path, budget)?;
    let after = match legacy {
        Attempt::Acquired(_) if warn => {
            After::Second(handles.acquire(Slot::Second, &toml_path, budget)?)
        }
        Attempt::Acquired(_) => After::None,
        Attempt::OpenFailed(_) | Attempt::StatFailed(_) => {
            // Native conversion and fallback stat are reached only here. Every
            // metadata kind means present; open/body-read capability is absent.
            let native = terminated(&toml_path, budget)?;
            let native = native_path(&native)?;
            budget.graph.paths(1)?;
            After::Fallback(match handles.system.stat(native)? {
                Stat::Metadata(_) => Exists::Present,
                Stat::Error(code) => Exists::Failed(native_error(code)?),
            })
        }
    };
    let transcript = Transcript::checked(filename, warn, legacy, after)
        .ok_or(Error::Encoding("invalid live discovery transcript"))?;
    let diagnostic = transcript.diagnostic();
    if diagnostic != Diagnostic::None {
        // Terminal-only work is removed, not the source lifetime boundary.
        // Successful handles still overlap here; failed fstat already closed.
        handles.system.diagnostic(diagnostic);
    }
    drop(handles);
    observed::Record {
        query: observed::Query::Discovery {
            directory,
            filename,
            warn,
        },
        value: observed::Value::Discovery(transcript),
    }
    .checked()
}

fn native_error(code: i32) -> Result<NativeError> {
    if code <= 0 {
        return Err(Error::Encoding("discovery native error must be positive"));
    }
    Ok(NativeError::Os(code))
}

fn native_path(bytes: &[u8]) -> Result<&CStr> {
    CStr::from_bytes_with_nul(bytes).map_err(|_| Error::Encoding("invalid discovery native path"))
}

/// Pinned Unix PathBuf::_push for a fixed nonempty relative single filename:
/// append one separator only if the directory's last byte is not already '/'.
/// No Unicode, component normalization, type probe or whole-path cap is added.
fn joined(directory: &[u8], filename: &[u8], budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let separator = usize::from(!directory.ends_with(b"/"));
    let length = directory
        .len()
        .checked_add(separator)
        .and_then(|length| length.checked_add(filename.len()))
        .ok_or(Error::Encoding("discovery joined path length overflow"))?;
    let mut bytes = owned(length, budget)?;
    bytes.extend_from_slice(directory);
    if separator != 0 {
        bytes.push(b'/');
    }
    bytes.extend_from_slice(filename);
    Ok(bytes)
}

fn terminated(path: &[u8], budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let length = path
        .len()
        .checked_add(1)
        .ok_or(Error::Encoding("discovery native path length overflow"))?;
    let mut bytes = owned(length, budget)?;
    bytes.extend_from_slice(path);
    bytes.push(0);
    Ok(bytes)
}

fn owned(length: usize, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    budget.allocation.items(1)?;
    budget.allocation.bytes(length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length)?;
    Ok(bytes)
}
