//! Replay one whole role-specific read; no descriptor is reconstructed.

use std::ffi::CStr;

use super::{observed, Host};
use crate::input::membership::{git_read, Budget, Error, Result};
use observed::git_read::{Event, Role, Transcript};

struct Replay<'image> {
    transcript: &'image Transcript,
    event: usize,
    byte: usize,
    opened: bool,
}

impl Replay<'_> {
    fn next(&mut self) -> Result<Event> {
        let event = *self
            .transcript
            .events()
            .get(self.event)
            .ok_or(Error::Unobserved)?;
        self.event += 1;
        Ok(event)
    }
}

impl git_read::Source for Replay<'_> {
    fn open(&mut self, _path: &CStr, _budget: &mut Budget<'_>) -> Result<git_read::Call<()>> {
        let Event::Open(result) = self.next()? else {
            return Err(Error::ObservationType);
        };
        self.opened = result.is_ok();
        Ok(result)
    }
    fn length(&mut self, _budget: &mut Budget<'_>) -> Result<git_read::Call<u64>> {
        let Event::Length(result) = self.next()? else {
            return Err(Error::ObservationType);
        };
        Ok(result)
    }
    fn position(&mut self, _budget: &mut Budget<'_>) -> Result<git_read::Call<u64>> {
        let Event::Position(result) = self.next()? else {
            return Err(Error::ObservationType);
        };
        Ok(result)
    }
    fn read(
        &mut self,
        bytes: &mut [u8],
        _budget: &mut Budget<'_>,
    ) -> Result<git_read::Call<usize>> {
        let Event::Read { requested, result } = self.next()? else {
            return Err(Error::ObservationType);
        };
        if requested != bytes.len() {
            return Err(Error::Encoding(
                "Git read request differs from recorded source extent",
            ));
        }
        if let Ok(count) = result {
            let end = self
                .byte
                .checked_add(count)
                .ok_or(Error::Encoding("Git replay byte overflow"))?;
            let source = self
                .transcript
                .bytes()
                .get(self.byte..end)
                .ok_or(Error::Unobserved)?;
            let destination = bytes.get_mut(..count).ok_or(Error::ObservationType)?;
            destination.copy_from_slice(source);
            self.byte = end;
        }
        Ok(result)
    }
    fn directory(
        &mut self,
        _path: &CStr,
        _budget: &mut Budget<'_>,
    ) -> Result<git_read::Call<bool>> {
        let Event::Directory(result) = self.next()? else {
            return Err(Error::ObservationType);
        };
        Ok(result)
    }
    fn failed_bytes(&mut self, bytes: &[u8], _budget: &mut Budget<'_>) -> Result<()> {
        if bytes != self.transcript.bytes() {
            return Err(Error::ObservationType);
        }
        Ok(())
    }
    fn close(&mut self) {
        self.opened = false;
    }
}

impl Host<'_, '_> {
    pub(in super::super) fn git_buffer(&mut self) -> Result<git_read::Buffer> {
        git_read::Buffer::with_capacity(512, &mut self.budget)
    }
    pub(in super::super) fn git_configuration<T>(
        &mut self,
        source_path: std::path::PathBuf,
        bytes: &mut git_read::Buffer,
        complete: impl FnOnce(
            git_read::Configuration,
            &mut Option<std::path::PathBuf>,
            &mut git_read::Buffer,
            &mut Budget<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        use std::os::unix::ffi::OsStrExt;
        let path =
            observed::Path::from_bytes(source_path.as_os_str().as_bytes(), &mut self.budget)?;
        let role = Role::Configuration {
            capacity: bytes.capacity(),
        };
        let record = self.lookup(&observed::Query::GitRead { path, role })?;
        let observed::Value::GitRead(transcript) = &self.image.records()[record].value else {
            return Err(Error::ObservationType);
        };
        let mut replay = Replay {
            transcript,
            event: 0,
            byte: 0,
            opened: false,
        };
        let mut source_path = Some(source_path);
        let result = git_read::configuration(
            path_bytes(&self.image.records()[record].query)?,
            bytes,
            &mut self.budget,
            &mut replay,
            |outcome, bytes, budget| complete(outcome, &mut source_path, bytes, budget),
        )?;
        if replay.opened
            || replay.event != transcript.events().len()
            || replay.byte != transcript.bytes().len()
        {
            return Err(Error::Unconsumed);
        }
        self.used[record] = true;
        Ok(result)
    }
    pub(in super::super) fn git_read(
        &mut self,
        path: &[u8],
        role: Role,
    ) -> Result<git_read::Outcome> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        let record = self.lookup(&observed::Query::GitRead { path, role })?;
        let observed::Value::GitRead(transcript) = &self.image.records()[record].value else {
            return Err(Error::ObservationType);
        };
        let mut replay = Replay {
            transcript,
            event: 0,
            byte: 0,
            opened: false,
        };
        let outcome = git_read::run(
            path_bytes(&self.image.records()[record].query)?,
            role,
            &mut self.budget,
            &mut replay,
        )?;
        if replay.opened
            || replay.event != transcript.events().len()
            || replay.byte != transcript.bytes().len()
        {
            return Err(Error::Unconsumed);
        }
        self.used[record] = true;
        Ok(outcome)
    }
}

fn path_bytes(query: &observed::Query) -> Result<&[u8]> {
    match query {
        observed::Query::GitRead { path, .. } => Ok(path.bytes()),
        _ => Err(Error::ObservationType),
    }
}
