//! The immutable host refuses observations outside its exact typed image.

use super::{observed, stored, Budget, Error, Result};
mod git_read;
pub(super) mod stream;
pub(super) mod traversal;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod whole_file;

#[cfg(test)]
mod discovery;

#[cfg(test)]
mod canonical;

pub(super) struct Host<'image, 'budget> {
    image: &'image stored::Stored,
    used: Vec<bool>,
    budget: Budget<'budget>,
    streams: usize,
    scope: stream::Scope,
    next_traversal: u64,
}

/// All image records were used; this is not selector or current-source proof.
pub(super) struct Consumed<'image> {
    image: &'image stored::Stored,
}

/// Borrowed bytes from an EOF-terminated whole-file observation.
/// This proves neither producer correctness nor current freshness or origin.
#[derive(Debug)]
pub(super) struct File<'image> {
    bytes: &'image [u8],
}

impl<'image> File<'image> {
    pub(super) fn bytes(&self) -> &'image [u8] {
        self.bytes
    }
}

impl<'image, 'budget> Host<'image, 'budget> {
    pub(super) fn from_image(
        image: &'image stored::Stored,
        mut budget: Budget<'budget>,
    ) -> Result<Self> {
        let mut used = Vec::new();
        budget.reserve(&mut used, image.records().len())?;
        used.resize(image.records().len(), false);
        Ok(Self {
            image,
            used,
            budget,
            streams: 0,
            scope: stream::Scope::new()?,
            next_traversal: 0,
        })
    }

    pub(super) fn observe(&mut self, query: &observed::Query) -> Result<&'image observed::Value> {
        let index = self.lookup(query)?;
        self.used[index] = true;
        Ok(&self.image.records()[index].value)
    }

    fn lookup(&mut self, query: &observed::Query) -> Result<usize> {
        // Every lookup consumes current graph work, including repeated requests.
        if matches!(
            query,
            observed::Query::Metadata { .. }
                | observed::Query::Read { .. }
                | observed::Query::Link(_)
                | observed::Query::Directory(_)
                | observed::Query::File(_)
                | observed::Query::Discovery { .. }
                | observed::Query::Canonical(_)
                | observed::Query::DirectoryStream(_)
                | observed::Query::Traversal { .. }
                | observed::Query::GitRead { .. }
        ) {
            self.budget.graph.paths(1)?;
        }
        let records = self.image.records();
        let mut start = 0;
        let mut end = records.len();
        while start < end {
            let mid = start + (end - start) / 2;
            match records[mid].query.cmp(query) {
                std::cmp::Ordering::Less => start = mid + 1,
                std::cmp::Ordering::Greater => end = mid,
                std::cmp::Ordering::Equal => {
                    return Ok(mid);
                }
            }
        }
        Err(Error::Unobserved)
    }

    pub(super) fn metadata(
        &mut self,
        path: &[u8],
        follow: bool,
    ) -> Result<Option<observed::Metadata>> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::Metadata { path, follow })? {
            observed::Value::Absent => Ok(None),
            observed::Value::Error(code) => Err(Error::Io(*code)),
            observed::Value::Metadata(metadata) => Ok(Some(*metadata)),
            _ => Err(Error::ObservationType),
        }
    }

    /// Borrow the exact recorded range; zero-length bytes are not absence.
    /// Success does not establish whole-file EOF or exclude later bytes.
    pub(super) fn read(
        &mut self,
        path: &[u8],
        offset: u64,
        length: u64,
    ) -> Result<Option<&'image [u8]>> {
        offset
            .checked_add(length)
            .ok_or(Error::Encoding("read range overflow"))?;
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::Read {
            path,
            offset,
            length,
        })? {
            observed::Value::Absent => Ok(None),
            observed::Value::Error(code) => Err(Error::Io(*code)),
            observed::Value::Bytes(bytes) => Ok(Some(bytes)),
            _ => Err(Error::ObservationType),
        }
    }

    /// Borrow complete raw file bytes only after a recorded successful EOF.
    /// Read errors take precedence over parsing or UTF-8 errors in any prefix.
    /// The independently qualified producer must establish contiguous same-stream
    /// acquisition and an actual nonempty EOF probe; this data host cannot do so.
    pub(super) fn file(&mut self, path: &[u8]) -> Result<Option<File<'image>>> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::File(path))? {
            observed::Value::Absent => Ok(None),
            observed::Value::Error(code)
            | observed::Value::File {
                end: observed::End::Error(code),
                ..
            } => Err(Error::Io(*code)),
            observed::Value::File {
                bytes,
                end: observed::End::Eof,
            } => Ok(Some(File { bytes })),
            _ => Err(Error::ObservationType),
        }
    }

    /// Borrow native target bytes without resolving or normalizing the link.
    pub(super) fn link(&mut self, path: &[u8]) -> Result<Option<&'image [u8]>> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::Link(path))? {
            observed::Value::Absent => Ok(None),
            observed::Value::Error(code) => Err(Error::Io(*code)),
            observed::Value::Link(target) => Ok(Some(target)),
            _ => Err(Error::ObservationType),
        }
    }

    /// An observed frontier always includes its terminal EOF or error.
    /// A partial error frontier remains distinct from failure before entries.
    pub(super) fn directory(
        &mut self,
        path: &[u8],
    ) -> Result<Option<(&'image [observed::Entry], observed::End)>> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::Directory(path))? {
            observed::Value::Absent => Ok(None),
            observed::Value::Error(code) => Err(Error::Io(*code)),
            observed::Value::Directory { entries, end } => Ok(Some((entries, *end))),
            _ => Err(Error::ObservationType),
        }
    }

    /// Read Cargo's sealed command environment, never Rawr's ambient process.
    pub(super) fn environment(&mut self, key: &[u8]) -> Result<Option<&'image [u8]>> {
        let key = self.budget.string(key)?;
        match self.observe(&observed::Query::Environment(key))? {
            observed::Value::Environment(value) => Ok(value.as_deref()),
            _ => Err(Error::ObservationType),
        }
    }

    /// The effective uid of the sealed command, never the ambient Rawr process.
    pub(super) fn identity(&mut self) -> Result<u32> {
        self.budget.allocation.items(1)?;
        match self.observe(&observed::Query::Identity)? {
            observed::Value::Identity { effective_uid } => Ok(*effective_uid),
            _ => Err(Error::ObservationType),
        }
    }

    /// A negative result must be recorded; missing evidence is never absence.
    pub(super) fn environment_prefix(&mut self, prefix: &[u8]) -> Result<bool> {
        let prefix = self.budget.string(prefix)?;
        match self.observe(&observed::Query::EnvironmentPrefix(prefix))? {
            observed::Value::EnvironmentPrefix(present) => Ok(*present),
            _ => Err(Error::ObservationType),
        }
    }

    pub(super) fn budget(&mut self) -> &mut Budget<'budget> {
        &mut self.budget
    }

    /// Borrow checked recorded native output. Only inner Err is a native
    /// canonicalization failure; all outer failures remain terminal to replay.
    pub(super) fn canonicalize(
        &mut self,
        path: &[u8],
    ) -> Result<std::result::Result<&'image [u8], i32>> {
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::Canonical(path))? {
            observed::Value::Canonical(Ok(path)) => Ok(Ok(path.bytes())),
            observed::Value::Canonical(Err(code)) => Ok(Err(*code)),
            _ => Err(Error::ObservationType),
        }
    }

    /// Return checked recorded acquisition facts, never current authority.
    /// Missing acquisition evidence cannot stand in for no selected filename.
    pub(super) fn discovery(
        &mut self,
        directory: &[u8],
        filename: observed::discovery::Filename,
        warn: bool,
    ) -> Result<observed::discovery::Transcript> {
        let directory = observed::Path::from_bytes(directory, &mut self.budget)?;
        match self.observe(&observed::Query::Discovery {
            directory,
            filename,
            warn,
        })? {
            observed::Value::Discovery(transcript) => Ok(*transcript),
            _ => Err(Error::ObservationType),
        }
    }

    pub(super) fn finish(self) -> Result<Consumed<'image>> {
        if self.streams != 0 || self.used.iter().any(|used| !used) {
            return Err(Error::Unconsumed);
        }
        Ok(Consumed { image: self.image })
    }
}

impl Consumed<'_> {
    pub(super) fn image(&self) -> &stored::Stored {
        self.image
    }
}
