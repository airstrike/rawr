// Copyright the Rust Project Developers and gitoxide contributors.
// Licensed under MIT OR Apache-2.0; see manifest/load/selection/git/LICENSE-*.
//! Pinned Git reads share one bounded source algorithm for live/replay.
//!
//! Pinned nightly-2026-07-11 std fs.rs:1312,1375 and io/mod.rs:418–529;
//! sys/fd/unix.rs:181–201 and core/io/borrowed_buf.rs:304–307 establish the
//! native read/init behavior. gix-ref 0.65.0 store/file/find.rs:280–318 supplies
//! the reference lifetime and 128-byte initial buffer; gix-discover 0.53.0
//! path.rs:29–45 supplies the descriptor gate and 512-byte initial buffer.
//! This adapter uses initialized storage and fallible, prospectively admitted
//! replacement layouts. It does not claim allocator-internal capacity identity.

use std::{
    ffi::{CStr, OsStr},
    os::unix::ffi::OsStrExt,
    path::Path,
};

use super::{
    observed::git_read::{self, Role},
    Budget, Error, Result,
};

pub(super) type Call<T> = std::result::Result<T, i32>;

/// The source operations of this closed read only; no handle escapes it.
pub(super) trait Source {
    fn open(&mut self, path: &CStr, budget: &mut Budget<'_>) -> Result<Call<()>>;
    fn length(&mut self, budget: &mut Budget<'_>) -> Result<Call<u64>>;
    fn position(&mut self, budget: &mut Budget<'_>) -> Result<Call<u64>>;
    fn read(&mut self, bytes: &mut [u8], budget: &mut Budget<'_>) -> Result<Call<usize>>;
    fn directory(&mut self, path: &CStr, budget: &mut Budget<'_>) -> Result<Call<bool>>;
    fn failed_bytes(&mut self, bytes: &[u8], budget: &mut Budget<'_>) -> Result<()>;
    fn close(&mut self);
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    Bytes(Vec<u8>),
    OpenFailed(i32),
    GateFailed(i32),
    /// Complete source-owned error, constructed while the descriptor lives.
    Oversized(super::manifest::io::Failure),
    ReadFailed {
        code: i32,
        directory: Option<Call<bool>>,
    },
}

struct Opened<'source, S: Source>(&'source mut S);
impl<S: Source> Drop for Opened<'_, S> {
    fn drop(&mut self) {
        self.0.close();
    }
}

pub(super) fn run(
    path: &[u8],
    role: Role,
    budget: &mut Budget<'_>,
    source: &mut impl Source,
) -> Result<Outcome> {
    {
        let native = terminated(path, budget)?;
        let native = native_path(&native)?;
        // Source File drop is one-shot and cannot be prevented by later refusal.
        budget.graph.paths(1)?;
        loop {
            budget.graph.paths(1)?;
            match source.open(native, budget)? {
                Ok(()) => break,
                Err(code) => {
                    git_read::errno(code)?;
                    if code != nix::libc::EINTR {
                        return Ok(Outcome::OpenFailed(code));
                    }
                }
            }
        }
    }
    let mut source = Opened(source);
    if role == Role::DiscoveryPath {
        budget.graph.paths(1)?;
        match source.0.length(budget)? {
            Ok(length) if length > 65536 => {
                let error = super::manifest::io::oversized(
                    Path::new(OsStr::from_bytes(path)),
                    length,
                    budget,
                )?;
                return Ok(Outcome::Oversized(error));
            }
            Ok(_) => {}
            Err(code) => {
                git_read::errno(code)?;
                return Ok(Outcome::GateFailed(code));
            }
        }
    }
    // Source allocates this vector only after open and the optional size gate.
    let capacity = match role {
        Role::LooseReference => 128,
        Role::DiscoveryPath => 512,
        Role::Configuration { .. } => {
            return Err(Error::Encoding("configuration requires retained buffer"))
        }
    };
    let mut bytes = Buffer::with_capacity(capacity, budget)?;
    budget.graph.paths(1)?;
    let hint = match source.0.length(budget)? {
        Err(code) => {
            git_read::errno(code)?;
            None
        }
        Ok(length) => {
            budget.graph.paths(1)?;
            match source.0.position(budget)? {
                Err(code) => {
                    git_read::errno(code)?;
                    None
                }
                Ok(position) => Some(length.saturating_sub(position) as usize),
            }
        }
    };
    bytes.reserve(hint.unwrap_or(0), budget)?;
    match read_to_end(&mut source, &mut bytes, hint, budget)? {
        Ok(()) => Ok(Outcome::Bytes(bytes.into_values())),
        Err(code) => {
            // ref_contents probes while its File and failed content Vec coexist.
            let directory = if role == Role::LooseReference {
                let native = terminated(path, budget)?;
                budget.graph.paths(1)?;
                let result = source.0.directory(native_path(&native)?, budget)?;
                if let Err(code) = result {
                    git_read::errno(code)?;
                }
                Some(result)
            } else {
                None
            };
            source.0.failed_bytes(bytes.as_slice(), budget)?;
            // Evidence has separate admitted ownership. The source Vec retires
            // before File on this error branch, even though evidence survives.
            drop(bytes);
            Ok(Outcome::ReadFailed { code, directory })
        }
    }
}

fn read_to_end<S: Source>(
    source: &mut Opened<'_, S>,
    bytes: &mut Buffer,
    hint: Option<usize>,
    budget: &mut Budget<'_>,
) -> Result<Call<()>> {
    let start_capacity = bytes.capacity;
    let mut maximum = hint
        .and_then(|size| size.checked_add(1024)?.checked_next_multiple_of(8192))
        .unwrap_or(8192);
    // These source roles begin with at least 128 spare bytes, so std's initial
    // spare<32 probe is unreachable. The exact-fit probe inside the loop is not.
    loop {
        if bytes.length == bytes.capacity && bytes.capacity == start_capacity {
            let mut probe = [0u8; 32];
            let count = match read(source, &mut probe, budget)? {
                Ok(0) => return Ok(Ok(())),
                Ok(count) => count,
                Err(code) => return Ok(Err(code)),
            };
            bytes.reserve(count, budget)?;
            bytes.values[bytes.length..bytes.length + count].copy_from_slice(&probe[..count]);
            bytes.length += count;
        }
        if bytes.length == bytes.capacity {
            bytes.reserve(32, budget)?;
        }
        let requested = (bytes.capacity - bytes.length).min(maximum);
        let count = match read(
            source,
            &mut bytes.values[bytes.length..bytes.length + requested],
            budget,
        )? {
            Ok(count) => count,
            Err(code) => return Ok(Err(code)),
        };
        bytes.length += count;
        if count == 0 {
            return Ok(Ok(()));
        }
        if hint.is_none() {
            // Unix FileDesc::read_buf advances filled, never the BorrowedBuf
            // initialization marker. Physical initialization in this safe
            // adapter does not change that source fact, even on a full read.
            maximum = usize::MAX;
        }
    }
}

fn read<S: Source>(
    source: &mut Opened<'_, S>,
    bytes: &mut [u8],
    budget: &mut Budget<'_>,
) -> Result<Call<usize>> {
    loop {
        budget.graph.paths(1)?;
        match source.0.read(bytes, budget)? {
            Ok(count) if count <= bytes.len() => return Ok(Ok(count)),
            Ok(_) => return Err(Error::Encoding("Git native read exceeded requested extent")),
            Err(code) => {
                git_read::errno(code)?;
                if code != nix::libc::EINTR {
                    return Ok(Err(code));
                }
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct Buffer {
    values: Vec<u8>,
    length: usize,
    capacity: usize,
}
impl Buffer {
    pub(super) fn with_capacity(capacity: usize, budget: &mut Budget<'_>) -> Result<Self> {
        budget.allocation.items(1)?;
        budget.allocation.bytes(capacity)?;
        let mut values = Vec::new();
        values.try_reserve_exact(capacity)?;
        values.resize(capacity, 0);
        Ok(Self {
            values,
            length: 0,
            capacity,
        })
    }
    fn reserve(&mut self, additional: usize, budget: &mut Budget<'_>) -> Result<()> {
        let required = self
            .length
            .checked_add(additional)
            .ok_or(Error::Encoding("Git read capacity overflow"))?;
        if required <= self.capacity {
            return Ok(());
        }
        let capacity = self
            .capacity
            .checked_mul(2)
            .ok_or(Error::Encoding("Git read growth overflow"))?
            .max(required)
            .max(8);
        // Match Vec<u8>'s requested amortized layout, not allocator bonus space.
        // Charge the full replacement while the old allocation is still live.
        budget.allocation.bytes(capacity)?;
        let mut replacement = Vec::new();
        replacement.try_reserve_exact(capacity)?;
        replacement.resize(capacity, 0);
        replacement[..self.length].copy_from_slice(self.as_slice());
        self.values = replacement;
        self.capacity = capacity;
        Ok(())
    }
    fn as_slice(&self) -> &[u8] {
        &self.values[..self.length]
    }
    pub(super) fn capacity(&self) -> usize {
        self.capacity
    }
    pub(super) fn clear(&mut self) {
        self.length = 0;
    }
    pub(super) fn bytes(&self) -> &[u8] {
        self.as_slice()
    }
    fn into_values(mut self) -> Vec<u8> {
        self.values.truncate(self.length);
        self.values
    }
}

#[derive(Debug)]
pub(super) enum Configuration {
    OpenFailed(i32),
    Read(Call<()>),
}

/// The config buffer predates open and survives the operation. Completion is
/// inside the File scope, so parsing/refusal/error owners coexist with its fd.
pub(super) fn configuration<T>(
    path: &[u8],
    bytes: &mut Buffer,
    budget: &mut Budget<'_>,
    source: &mut impl Source,
    complete: impl FnOnce(Configuration, &mut Buffer, &mut Budget<'_>) -> Result<T>,
) -> Result<T> {
    if bytes.capacity < 512 {
        return Err(Error::Encoding("invalid retained configuration capacity"));
    }
    let opened = {
        let native = terminated(path, budget)?;
        let native = native_path(&native)?;
        budget.graph.paths(1)?; // one unconditional cleanup allowance
        loop {
            budget.graph.paths(1)?;
            match source.open(native, budget)? {
                Ok(()) => break Ok(()),
                Err(code) => {
                    git_read::errno(code)?;
                    if code != nix::libc::EINTR {
                        break Err(code);
                    }
                }
            }
        }
    };
    if let Err(code) = opened {
        return complete(Configuration::OpenFailed(code), bytes, budget);
    }
    let mut source = Opened(source);
    bytes.clear();
    budget.graph.paths(1)?;
    let hint = match source.0.length(budget)? {
        Err(code) => {
            git_read::errno(code)?;
            None
        }
        Ok(length) => {
            budget.graph.paths(1)?;
            match source.0.position(budget)? {
                Err(code) => {
                    git_read::errno(code)?;
                    None
                }
                Ok(position) => Some(length.saturating_sub(position) as usize),
            }
        }
    };
    bytes.reserve(hint.unwrap_or(0), budget)?;
    let result = read_to_end(&mut source, bytes, hint, budget)?;
    // The evidence copy has its own admitted lifetime; completion may clear
    // the source buffer on a lenient error while that original fd remains live.
    source.0.failed_bytes(bytes.as_slice(), budget)?;
    complete(Configuration::Read(result), bytes, budget)
}

fn terminated(path: &[u8], budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let length = path
        .len()
        .checked_add(1)
        .ok_or(Error::Encoding("Git native path length overflow"))?;
    budget.allocation.items(1)?;
    budget.allocation.bytes(length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length)?;
    bytes.extend_from_slice(path);
    bytes.push(0);
    Ok(bytes)
}
fn native_path(bytes: &[u8]) -> Result<&CStr> {
    CStr::from_bytes_with_nul(bytes).map_err(|_| Error::Encoding("invalid Git native path"))
}
