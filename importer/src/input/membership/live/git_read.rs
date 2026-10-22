//! Closed Git file acquisition, preserving descriptor and failed-buffer scopes.

use std::ffi::CStr;

use super::{file, Stat};
use crate::input::membership::{git_read, observed, Budget, Error, Result};
use observed::git_read::{Event, Role, Transcript};

pub(super) trait System: file::System {
    fn position(&mut self, descriptor: i32) -> git_read::Call<u64>;
}

struct Capture<'system, S: System> {
    system: &'system mut S,
    descriptor: Option<i32>,
    events: Vec<Event>,
    admitted: usize,
    partial: Vec<u8>,
}

impl<S: System> Capture<'_, S> {
    fn prepare(&mut self, budget: &mut Budget<'_>) -> Result<()> {
        budget.allocation.items(1)?;
        if self.events.len() == self.admitted {
            let capacity = self
                .admitted
                .max(2)
                .checked_mul(2)
                .ok_or(Error::Encoding("Git read event capacity overflow"))?;
            let layout = std::alloc::Layout::array::<Event>(capacity)
                .map_err(|_| Error::Encoding("Git read event layout overflow"))?;
            budget.allocation.bytes(layout.size())?;
            let mut replacement = Vec::new();
            replacement.try_reserve_exact(capacity)?;
            replacement.append(&mut self.events);
            self.events = replacement;
            self.admitted = capacity;
        }
        Ok(())
    }
    fn descriptor(&self) -> Result<i32> {
        self.descriptor
            .ok_or(Error::Encoding("Git operation without open descriptor"))
    }
}

impl<S: System> git_read::Source for Capture<'_, S> {
    fn open(&mut self, path: &CStr, budget: &mut Budget<'_>) -> Result<git_read::Call<()>> {
        self.prepare(budget)?;
        let result = match self.system.open(path) {
            Ok(descriptor) if descriptor >= 0 => {
                self.descriptor = Some(descriptor);
                Ok(())
            }
            Ok(_) => return Err(Error::Encoding("Git open returned negative descriptor")),
            Err(code) => {
                observed::git_read::errno(code)?;
                Err(code)
            }
        };
        self.events.push(Event::Open(result));
        Ok(result)
    }
    fn length(&mut self, budget: &mut Budget<'_>) -> Result<git_read::Call<u64>> {
        self.prepare(budget)?;
        let result = match self.system.fstat(self.descriptor()?)? {
            Stat::Metadata(metadata) => Ok(metadata.length()),
            Stat::Error(code) => {
                observed::git_read::errno(code)?;
                Err(code)
            }
        };
        self.events.push(Event::Length(result));
        Ok(result)
    }
    fn position(&mut self, budget: &mut Budget<'_>) -> Result<git_read::Call<u64>> {
        self.prepare(budget)?;
        let result = self.system.position(self.descriptor()?);
        if let Err(code) = result {
            observed::git_read::errno(code)?;
        }
        self.events.push(Event::Position(result));
        Ok(result)
    }
    fn read(&mut self, bytes: &mut [u8], budget: &mut Budget<'_>) -> Result<git_read::Call<usize>> {
        self.prepare(budget)?;
        // Darwin's pinned FileDesc read/read_buf cap the actual syscall at INT_MAX.
        let extent = bytes.len().min(i32::MAX as usize);
        let result = self.system.read(self.descriptor()?, &mut bytes[..extent]);
        if let Err(code) = result {
            observed::git_read::errno(code)?;
        }
        if result.is_ok_and(|count| count > extent) {
            return Err(Error::Encoding("Git native read exceeds request"));
        }
        self.events.push(Event::Read {
            requested: bytes.len(),
            result,
        });
        Ok(result)
    }
    fn directory(&mut self, path: &CStr, budget: &mut Budget<'_>) -> Result<git_read::Call<bool>> {
        self.prepare(budget)?;
        let result = match self.system.stat(path)? {
            Stat::Metadata(metadata) => Ok(metadata.kind() == observed::Kind::Directory),
            Stat::Error(code) => {
                observed::git_read::errno(code)?;
                Err(code)
            }
        };
        self.events.push(Event::Directory(result));
        Ok(result)
    }
    fn failed_bytes(&mut self, bytes: &[u8], budget: &mut Budget<'_>) -> Result<()> {
        self.partial = budget.bytes(bytes)?;
        Ok(())
    }
    fn close(&mut self) {
        if let Some(descriptor) = self.descriptor.take() {
            self.system.close(descriptor);
        }
    }
}

impl<S: System> Drop for Capture<'_, S> {
    fn drop(&mut self) {
        if let Some(descriptor) = self.descriptor.take() {
            self.system.close(descriptor);
        }
    }
}

pub(super) fn acquire(
    path: &[u8],
    role: Role,
    budget: &mut Budget<'_>,
    system: &mut impl System,
) -> Result<observed::Record> {
    let path = observed::Path::from_bytes(path, budget)?;
    let mut capture = Capture {
        system,
        descriptor: None,
        events: Vec::new(),
        admitted: 0,
        partial: Vec::new(),
    };
    let outcome = git_read::run(path.bytes(), role, budget, &mut capture)?;
    let bytes = match outcome {
        git_read::Outcome::Bytes(bytes) => bytes,
        _ => std::mem::take(&mut capture.partial),
    };
    let transcript = Transcript::checked(role, std::mem::take(&mut capture.events), bytes)?;
    observed::Record {
        query: observed::Query::GitRead { path, role },
        value: observed::Value::GitRead(transcript),
    }
    .checked()
}

/// One actual source occurrence with its caller's retained buffer. The closed
/// completion runs while Capture owns the descriptor, before record sealing.
pub(super) fn configuration<T>(
    path: &[u8],
    buffer: &mut git_read::Buffer,
    budget: &mut Budget<'_>,
    system: &mut impl System,
    complete: impl FnOnce(git_read::Configuration, &mut git_read::Buffer, &mut Budget<'_>) -> Result<T>,
) -> Result<(observed::Record, T)> {
    let path = observed::Path::from_bytes(path, budget)?;
    let role = Role::Configuration {
        capacity: buffer.capacity(),
    };
    let mut capture = Capture {
        system,
        descriptor: None,
        events: Vec::new(),
        admitted: 0,
        partial: Vec::new(),
    };
    let value = git_read::configuration(path.bytes(), buffer, budget, &mut capture, complete)?;
    let transcript = Transcript::checked(
        role,
        std::mem::take(&mut capture.events),
        std::mem::take(&mut capture.partial),
    )?;
    let record = observed::Record {
        query: observed::Query::GitRead { path, role },
        value: observed::Value::GitRead(transcript),
    }
    .checked()?;
    Ok((record, value))
}

#[cfg(test)]
pub(in crate::input::membership) mod tests;
