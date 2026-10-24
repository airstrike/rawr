use std::ffi::CStr;

use super::{Acquired, Identity, Stat};
use crate::input::membership::{observed, Budget, Error, Result};

pub(super) const CHUNK: usize = 8192;

#[cfg(test)]
thread_local! {
    pub(super) static RESERVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

type Call<T> = std::result::Result<T, i32>;

/// Private syscall seam. Both native calls and deterministic fixtures execute
/// the acquisition state machine below; the seam never receives a budget.
pub(super) trait System {
    fn stat(&mut self, path: &CStr) -> Result<Stat>;
    fn open(&mut self, path: &CStr) -> Call<i32>;
    fn fstat(&mut self, descriptor: i32) -> Result<Stat>;
    fn read(&mut self, descriptor: i32, bytes: &mut [u8]) -> Call<usize>;
    fn close(&mut self, descriptor: i32);
}

/// Born only from a successful open; never cloned, exposed, sought or retargeted.
struct Descriptor<'system, S: System> {
    descriptor: i32,
    system: &'system mut S,
}

impl<S: System> Drop for Descriptor<'_, S> {
    fn drop(&mut self) {
        // Cleanup was admitted before open. Refusal cannot suppress it, and a
        // close error is never retried against a potentially reused descriptor.
        self.system.close(self.descriptor);
    }
}

pub(super) fn acquire(
    path: &[u8],
    limit: usize,
    budget: &mut Budget<'_>,
    system: &mut impl System,
) -> Result<Acquired> {
    let path = observed::Path::from_bytes(path, budget)?;
    let native = terminated(path.bytes(), budget)?;
    let native = CStr::from_bytes_with_nul(&native)
        .map_err(|_| Error::Encoding("invalid terminated native path"))?;

    budget.graph.paths(1)?;
    let before_path = system.stat(native)?;
    // Reserve exactly one possible descriptor cleanup before any acquisition.
    // A failed open leaves this reservation spent, like every other admission.
    budget.graph.paths(1)?;
    let opened = loop {
        budget.graph.paths(1)?;
        match system.open(native) {
            Err(code) if code == libc::EINTR => continue,
            result => break result,
        }
    };

    let (value, opened, after_opened) = match opened {
        Err(code) => {
            let value = if code == libc::ENOENT {
                observed::Value::Absent
            } else {
                observed::Value::Error(code)
            };
            (value, None, None)
        }
        Ok(descriptor) => {
            let mut source = Descriptor {
                descriptor,
                system: &mut *system,
            };
            budget.graph.paths(1)?;
            let opened = source.system.fstat(source.descriptor)?;
            let value = read(&mut source, limit, budget)?;
            budget.graph.paths(1)?;
            let after = source.system.fstat(source.descriptor)?;
            (value, Some(opened), Some(after))
        }
    };
    budget.graph.paths(1)?;
    let after_path = system.stat(native)?;
    let record = observed::Record {
        query: observed::Query::File(path),
        value,
    }
    .checked()?;
    Ok(Acquired {
        record,
        identity: Identity {
            before_path,
            opened,
            after_opened,
            after_path,
        },
    })
}

fn terminated(path: &[u8], budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let length = path
        .len()
        .checked_add(1)
        .ok_or(Error::Encoding("native path length overflow"))?;
    budget.allocation.items(1)?;
    budget.allocation.bytes(length)?;
    let mut bytes = Vec::new();
    reserve(&mut bytes, length)?;
    bytes.extend_from_slice(path);
    bytes.push(0);
    Ok(bytes)
}

fn read<S: System>(
    source: &mut Descriptor<'_, S>,
    limit: usize,
    budget: &mut Budget<'_>,
) -> Result<observed::Value> {
    // Fixed stack storage has a static bound, not a requested heap layout.
    // Admitted read attempts times CHUNK bound initialization/read/copy work.
    let mut scratch = [0_u8; CHUNK];
    budget.allocation.items(1)?;
    let mut bytes = Bytes {
        values: Vec::new(),
        admitted: 0,
    };
    loop {
        let remaining = limit - bytes.values.len();
        let requested = if remaining == 0 {
            1
        } else {
            remaining.min(CHUNK)
        };
        budget.graph.paths(1)?;
        match source
            .system
            .read(source.descriptor, &mut scratch[..requested])
        {
            Ok(0) => {
                return Ok(observed::Value::File {
                    bytes: bytes.values,
                    end: observed::End::Eof,
                })
            }
            Ok(count) => {
                if count > requested {
                    return Err(Error::Encoding("file read exceeded its requested extent"));
                }
                let length = bytes
                    .values
                    .len()
                    .checked_add(count)
                    .ok_or(Error::Encoding("file length overflow"))?;
                super::super::super::limited("membership manifest bytes", limit, length)?;
                bytes.extend(&scratch[..count], limit, budget)?;
            }
            Err(code) if code == libc::EINTR => continue,
            Err(code) if bytes.values.is_empty() => return Ok(observed::Value::Error(code)),
            Err(code) => {
                return Ok(observed::Value::File {
                    bytes: bytes.values,
                    end: observed::End::Error(code),
                })
            }
        }
    }
}

struct Bytes {
    values: Vec<u8>,
    admitted: usize,
}

impl Bytes {
    fn extend(&mut self, bytes: &[u8], limit: usize, budget: &mut Budget<'_>) -> Result<()> {
        let required = self
            .values
            .len()
            .checked_add(bytes.len())
            .ok_or(Error::Encoding("file buffer length overflow"))?;
        if required > self.admitted {
            let grown = self
                .admitted
                .checked_mul(2)
                .ok_or(Error::Encoding("file buffer capacity overflow"))?;
            let capacity = required.max(grown.min(limit));
            // Charge the full replacement layout, never only the increment.
            // The previous layout remains spent. Allocator bonus capacity is
            // not permission to bypass this request or its admission.
            budget.allocation.bytes(capacity)?;
            let additional = capacity - self.values.len();
            reserve(&mut self.values, additional)?;
            self.admitted = capacity;
        }
        self.values.extend_from_slice(bytes);
        Ok(())
    }
}

fn reserve(bytes: &mut Vec<u8>, additional: usize) -> Result<()> {
    #[cfg(test)]
    RESERVATIONS.with(|count| count.set(count.get() + 1));
    bytes.try_reserve_exact(additional)?;
    Ok(())
}

#[cfg(test)]
pub(super) fn bonus_refusal(budget: &mut Budget<'_>) -> Result<()> {
    let mut bytes = Bytes {
        values: Vec::with_capacity(128),
        admitted: 1,
    };
    bytes.values.push(1);
    let result = bytes.extend(&[2], 128, budget);
    if result.is_err() {
        assert_eq!(bytes.values, [1]);
        assert_eq!(bytes.admitted, 1);
    }
    result
}
