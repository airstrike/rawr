//! Recorded source predicates and complete documents on the same Session.

use super::super::super::{archive, document, host, observed, Error as InputError};
use super::super::{Error, Step};
use super::Session;
use crate::cargo;

mod traversal;

impl<'image> Session<'_, host::Host<'image, '_>> {
    pub(in super::super) fn git_buffer(
        &mut self,
    ) -> Step<crate::input::membership::git_read::Buffer> {
        self.check()?;
        match self.admission.git_buffer() {
            Ok(buffer) => Ok(buffer),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    pub(in super::super) fn git_configuration<T>(
        &mut self,
        path: std::path::PathBuf,
        buffer: &mut crate::input::membership::git_read::Buffer,
        complete: impl FnOnce(
            crate::input::membership::git_read::Configuration,
            &mut Option<std::path::PathBuf>,
            &mut crate::input::membership::git_read::Buffer,
            &mut crate::input::membership::Budget<'_>,
        ) -> crate::input::membership::Result<T>,
    ) -> Step<T> {
        self.check()?;
        match self.admission.git_configuration(path, buffer, complete) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    /// Replay the complete closed read before decoding; source errno remains
    /// data, while missing evidence and original-account refusal latch here.
    pub(in super::super) fn git_read(
        &mut self,
        path: &[u8],
        role: observed::git_read::Role,
    ) -> Step<crate::input::membership::git_read::Outcome> {
        self.check()?;
        match self.admission.git_read(path, role) {
            Ok(outcome) => Ok(outcome),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    /// Project supplied selection facts under the original host/account. This
    /// does not establish that selection or command facts are authoritative.
    pub(in super::super) fn archive<B>(
        &mut self,
        inputs: archive::Inputs<'_, B>,
    ) -> Step<archive::Projection>
    where
        B: IntoIterator,
        B::Item: AsRef<[u8]>,
    {
        self.check()?;
        match archive::Projection::from_iter(inputs, self.admission) {
            Ok(projection) => Ok(projection),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    /// Complete successful read and UTF-8 conversion precede embedded gating.
    pub(in super::super) fn text(
        &mut self,
        path: &[u8],
        limits: &cargo::Limits,
    ) -> Step<&'image str> {
        self.check()?;
        let file = match self.admission.file(path) {
            Ok(Some(file)) => file,
            Ok(None) => return self.fail(Error::Host(InputError::Io(nix::libc::ENOENT))),
            Err(error) => return self.fail(Error::Host(error)),
        };
        // The manifest size is an accounting target recorded on the
        // original account; the file bytes were retained under the host's
        // physical read bound and are not refused here.
        let length = file.bytes().len();
        self.admission.budget().allocation.exceeded(
            "membership manifest bytes",
            limits.manifest_bytes,
            length,
        );
        match std::str::from_utf8(file.bytes()) {
            Ok(text) => Ok(text),
            Err(error) => self.fail(Error::Host(InputError::ManifestEncoding(error))),
        }
    }

    pub(in super::super) fn parse_text<'source>(
        &mut self,
        text: &'source str,
        limits: &cargo::Limits,
    ) -> Step<rawr_toml::document::Document<'source>> {
        self.check()?;
        match document::parse_text(text, limits, self.admission.budget()) {
            Ok(document) => Ok(document),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// The sealed Cargo environment, retaining the original account and latch.
    pub(in super::super) fn environment(&mut self, key: &[u8]) -> Step<Option<&'image [u8]>> {
        self.check()?;
        match self.admission.environment(key) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// Replay command identity while preserving the original admission latch.
    pub(in super::super) fn identity(&mut self) -> Step<u32> {
        self.check()?;
        match self.admission.identity() {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// Cargo's optional config getter probes a key plus trailing underscore.
    pub(in super::super) fn environment_prefix(&mut self, prefix: &[u8]) -> Step<bool> {
        self.check()?;
        match self.admission.environment_prefix(prefix) {
            Ok(present) => Ok(present),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// Checked physical discovery facts, under this Session's original latch.
    /// Source-local NUL behavior belongs to the selector, before this host query.
    pub(in super::super) fn discovery(
        &mut self,
        directory: &[u8],
        filename: observed::discovery::Filename,
        warn: bool,
    ) -> Step<observed::discovery::Transcript> {
        self.check()?;
        match self.admission.discovery(directory, filename, warn) {
            Ok(transcript) => Ok(transcript),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// Native failure remains data for Cargo's spelling fallback. Unobserved,
    /// malformed or refused evidence latches terminally and cannot select it.
    /// The caller handles source-local NUL conversion before this typed query.
    pub(in super::super) fn canonicalize(
        &mut self,
        path: &[u8],
    ) -> Step<std::result::Result<&'image [u8], i32>> {
        self.check()?;
        match self.admission.canonicalize(path) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// Match Path::is_file: follow links and treat only recorded OS errors as false.
    pub(in super::super) fn is_file(&mut self, path: &[u8]) -> Step<bool> {
        self.predicate(path, |metadata| metadata.kind == observed::Kind::File)
    }

    /// Match Path::is_dir with the same followed metadata/error semantics.
    pub(in super::super) fn is_dir(&mut self, path: &[u8]) -> Step<bool> {
        self.predicate(path, |metadata| metadata.kind == observed::Kind::Directory)
    }

    /// Match Path::exists, including all present metadata kinds.
    pub(in super::super) fn exists(&mut self, path: &[u8]) -> Step<bool> {
        self.predicate(path, |_| true)
    }

    fn predicate(
        &mut self,
        path: &[u8],
        present: impl FnOnce(observed::Metadata) -> bool,
    ) -> Step<bool> {
        self.check()?;
        match self.admission.metadata(path, true) {
            Ok(Some(metadata)) => Ok(present(metadata)),
            Ok(None) | Err(InputError::Io(_)) => Ok(false),
            Err(error) => self.fail(Error::Host(error)),
        }
    }

    /// EOF/read-error checking precedes parsing. This remains semantic data,
    /// not complete manifest loading, embedded expansion or current authority.
    pub(in super::super) fn document(
        &mut self,
        path: &[u8],
        limits: &cargo::Limits,
    ) -> Step<Option<rawr_toml::document::Document<'image>>> {
        self.check()?;
        let file = match self.admission.file(path) {
            Ok(Some(file)) => file,
            Ok(None) => return Ok(None),
            Err(error) => return self.fail(Error::Host(error)),
        };
        match document::parse(file, limits, self.admission.budget()) {
            Ok(document) => Ok(Some(document)),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
}

#[cfg(test)]
mod tests;

impl<'image> Session<'_, host::Host<'image, '_>> {
    pub(in super::super) fn directory_stream(
        &mut self,
        path: &[u8],
    ) -> Step<std::result::Result<host::stream::Cursor<'image>, i32>> {
        self.check()?;
        match self.admission.stream(path) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    pub(in super::super) fn stream_next<'cursor>(
        &mut self,
        cursor: &'cursor mut host::stream::Cursor<'image>,
    ) -> Step<host::stream::Next<'cursor, 'image>> {
        self.check()?;
        match cursor.next(self.admission) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    pub(in super::super) fn stream_inspect(
        &mut self,
        entry: host::stream::Entry<'_, 'image>,
    ) -> Step<std::result::Result<observed::Kind, i32>> {
        self.check()?;
        match entry.inspect(self.admission) {
            Ok(value) => Ok(value),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
    pub(in super::super) fn stream_finish(
        &mut self,
        cursor: host::stream::Cursor<'image>,
    ) -> Step<()> {
        self.check()?;
        match cursor.finish(self.admission) {
            Ok(()) => Ok(()),
            Err(error) => self.fail(Error::Host(error)),
        }
    }
}
