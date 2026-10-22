//! Canonical observation data; decoding cannot construct freshness authority.

use super::{observed, Budget, Error, Result, CARGO_COMMIT};
use observed::discovery::{After, Attempt, Exists, Filename, Identity, NativeError, Transcript};

const MAGIC: &[u8] = b"rawr-package-observations\0\x06";
mod git_read;
mod stream;
mod traversal;

#[cfg(test)]
mod whole_file;

#[cfg(test)]
mod discovery;

#[cfg(test)]
mod canonical;

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Stored {
    records: Vec<observed::Record>,
}

struct Decoder<'bytes, 'budget, 'account> {
    bytes: &'bytes [u8],
    cursor: usize,
    budget: &'budget mut Budget<'account>,
}

struct Encoder<'output> {
    output: Option<&'output mut Vec<u8>>,
    length: usize,
}

impl Stored {
    /// Retain caller data under this account; nested buffers are never adopted.
    pub(super) fn from_records(
        records: &[observed::Record],
        budget: &mut Budget<'_>,
    ) -> Result<Self> {
        let mut checked: Vec<observed::Record> = Vec::new();
        budget.reserve(&mut checked, records.len())?;
        for record in records {
            let record = retain(record, budget)?.checked()?;
            if let Some(previous) = checked.last()
                && previous.query >= record.query
            {
                return Err(Error::Encoding("queries are not sorted and unique"));
            }
            checked.push(record);
        }
        Ok(Self { records: checked })
    }

    pub(super) fn records(&self) -> &[observed::Record] {
        &self.records
    }

    pub(super) fn decode(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Self> {
        budget.graph.decode(bytes.len())?;
        let mut decoder = Decoder {
            bytes,
            cursor: 0,
            budget,
        };
        if decoder.take(MAGIC.len())? != MAGIC
            || decoder.take(CARGO_COMMIT.len())? != CARGO_COMMIT.as_bytes()
        {
            return Err(Error::Encoding("unknown observation schema or Cargo pin"));
        }
        let count = decoder.count()?;
        // Every record has at least a query tag and an answer tag.
        if count > decoder.remaining() / 2 {
            return Err(Error::Encoding("impossible record count"));
        }
        let mut records: Vec<observed::Record> = Vec::new();
        decoder.budget.reserve(&mut records, count)?;
        for _ in 0..count {
            let query = decoder.query()?;
            let value = decoder.value()?;
            let record = observed::Record { query, value }.checked()?;
            if let Some(previous) = records.last()
                && previous.query >= record.query
            {
                return Err(Error::Encoding("queries are not sorted and unique"));
            }
            records.push(record);
        }
        if decoder.remaining() != 0 {
            return Err(Error::Encoding("trailing observation bytes"));
        }
        Ok(Self { records })
    }

    pub(super) fn encode(&self, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
        let mut measure = Encoder {
            output: None,
            length: 0,
        };
        measure.image(self)?;
        let mut output = Vec::new();
        budget.allocation.items(1)?;
        budget.allocation.bytes(measure.length)?;
        output.try_reserve_exact(measure.length)?;
        Encoder {
            output: Some(&mut output),
            length: 0,
        }
        .image(self)?;
        Ok(output)
    }
}

fn retain(record: &observed::Record, budget: &mut Budget<'_>) -> Result<observed::Record> {
    let query = match &record.query {
        observed::Query::Metadata { path, follow } => observed::Query::Metadata {
            path: observed::Path::from_bytes(path.bytes(), budget)?,
            follow: *follow,
        },
        observed::Query::Read {
            path,
            offset,
            length,
        } => observed::Query::Read {
            path: observed::Path::from_bytes(path.bytes(), budget)?,
            offset: *offset,
            length: *length,
        },
        observed::Query::Link(path) => {
            observed::Query::Link(observed::Path::from_bytes(path.bytes(), budget)?)
        }
        observed::Query::Directory(path) => {
            observed::Query::Directory(observed::Path::from_bytes(path.bytes(), budget)?)
        }
        observed::Query::DirectoryStream(path) => {
            observed::Query::DirectoryStream(observed::Path::from_bytes(path.bytes(), budget)?)
        }
        observed::Query::Traversal {
            root,
            is_root,
            invocation,
        } => observed::Query::Traversal {
            root: observed::Path::from_bytes(root.bytes(), budget)?,
            is_root: *is_root,
            invocation: *invocation,
        },
        observed::Query::Environment(key) => observed::Query::Environment(budget.string(key)?),
        observed::Query::EnvironmentPrefix(key) => {
            observed::Query::EnvironmentPrefix(budget.string(key)?)
        }
        observed::Query::Identity => observed::Query::Identity,
        observed::Query::File(path) => {
            observed::Query::File(observed::Path::from_bytes(path.bytes(), budget)?)
        }
        observed::Query::Canonical(path) => {
            observed::Query::Canonical(observed::Path::from_bytes(path.bytes(), budget)?)
        }
        observed::Query::Discovery {
            directory,
            filename,
            warn,
        } => observed::Query::Discovery {
            directory: observed::Path::from_bytes(directory.bytes(), budget)?,
            filename: *filename,
            warn: *warn,
        },
        observed::Query::GitRead { path, role } => observed::Query::GitRead {
            path: observed::Path::from_bytes(path.bytes(), budget)?,
            role: *role,
        },
    };
    let value = match &record.value {
        observed::Value::Absent => observed::Value::Absent,
        observed::Value::Error(error) => observed::Value::Error(*error),
        observed::Value::Metadata(metadata) => observed::Value::Metadata(*metadata),
        // Reads use the aggregate ownership bound, not the per-string bound.
        observed::Value::Bytes(bytes) => observed::Value::Bytes(budget.bytes(bytes)?),
        observed::Value::Link(target) => observed::Value::Link(budget.string(target)?),
        observed::Value::Directory { entries, end } => {
            let mut retained = Vec::new();
            budget.reserve(&mut retained, entries.len())?;
            for entry in entries {
                retained.push(observed::Entry {
                    name: budget.string(&entry.name)?,
                    kind: entry.kind,
                    inode: entry.inode,
                });
            }
            observed::Value::Directory {
                entries: retained,
                end: *end,
            }
        }
        observed::Value::Environment(value) => observed::Value::Environment(match value {
            Some(value) => Some(budget.string(value)?),
            None => None,
        }),
        observed::Value::EnvironmentPrefix(present) => observed::Value::EnvironmentPrefix(*present),
        observed::Value::Identity { effective_uid } => observed::Value::Identity {
            effective_uid: *effective_uid,
        },
        observed::Value::File { bytes, end } => observed::Value::File {
            bytes: budget.bytes(bytes)?,
            end: *end,
        },
        observed::Value::Discovery(transcript) => observed::Value::Discovery(*transcript),
        observed::Value::Canonical(value) => observed::Value::Canonical(match value {
            Ok(path) => Ok(observed::Path::from_bytes(path.bytes(), budget)?),
            Err(code) => Err(*code),
        }),
        observed::Value::DirectoryStream(transcript) => {
            observed::Value::DirectoryStream(stream::retain(transcript, budget)?)
        }
        observed::Value::Traversal(transcript) => {
            observed::Value::Traversal(traversal::retain(transcript, budget)?)
        }
        observed::Value::GitRead(transcript) => {
            observed::Value::GitRead(git_read::retain(transcript, budget)?)
        }
    };
    Ok(observed::Record { query, value })
}

impl Decoder<'_, '_, '_> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.cursor
    }

    fn take(&mut self, length: usize) -> Result<&[u8]> {
        let end = self
            .cursor
            .checked_add(length)
            .ok_or(Error::Encoding("length overflow"))?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(Error::Encoding("truncated image"))?;
        self.cursor = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| Error::Encoding("u32"))?,
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| Error::Encoding("u64"))?,
        ))
    }

    fn count(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?).map_err(|_| Error::Encoding("count exceeds address space"))
    }

    fn bytes(&mut self) -> Result<Vec<u8>> {
        let length = self.count()?;
        if length > self.remaining() {
            return Err(Error::Encoding("truncated byte string"));
        }
        self.budget.allocation.items(1)?;
        self.budget.allocation.bytes(length)?;
        let mut owned = Vec::new();
        owned.try_reserve_exact(length)?;
        owned.extend_from_slice(self.take(length)?);
        Ok(owned)
    }

    fn string(&mut self) -> Result<Vec<u8>> {
        let length = self.count()?;
        if length > self.remaining() {
            return Err(Error::Encoding("truncated string"));
        }
        let start = self.cursor;
        self.cursor += length;
        self.budget.string(&self.bytes[start..self.cursor])
    }

    fn path(&mut self) -> Result<observed::Path> {
        let length = self.count()?;
        if length > self.remaining() {
            return Err(Error::Encoding("truncated path"));
        }
        let start = self.cursor;
        self.cursor += length;
        observed::Path::from_bytes(&self.bytes[start..self.cursor], self.budget)
    }

    fn query(&mut self) -> Result<observed::Query> {
        Ok(match self.byte()? {
            0 => observed::Query::Metadata {
                path: self.path()?,
                follow: false,
            },
            1 => observed::Query::Metadata {
                path: self.path()?,
                follow: true,
            },
            2 => observed::Query::Read {
                path: self.path()?,
                offset: self.u64()?,
                length: self.u64()?,
            },
            3 => observed::Query::Link(self.path()?),
            4 => observed::Query::Directory(self.path()?),
            5 => observed::Query::Environment(self.string()?),
            6 => observed::Query::Identity,
            7 => observed::Query::File(self.path()?),
            8 => observed::Query::EnvironmentPrefix(self.string()?),
            9 => observed::Query::Discovery {
                directory: self.path()?,
                filename: self.filename()?,
                warn: self.boolean()?,
            },
            10 => observed::Query::Canonical(self.path()?),
            11 => observed::Query::DirectoryStream(self.path()?),
            12 => observed::Query::Traversal {
                root: self.path()?,
                is_root: self.boolean()?,
                invocation: self.u64()?,
            },
            13 => observed::Query::GitRead {
                path: self.path()?,
                role: self.git_role()?,
            },
            _ => return Err(Error::Encoding("unknown query tag")),
        })
    }

    fn kind(&mut self) -> Result<observed::Kind> {
        Ok(match self.byte()? {
            0 => observed::Kind::File,
            1 => observed::Kind::Directory,
            2 => observed::Kind::Symlink,
            3 => observed::Kind::Fifo,
            4 => observed::Kind::Socket,
            5 => observed::Kind::Character,
            6 => observed::Kind::Block,
            7 => observed::Kind::Unknown,
            _ => return Err(Error::Encoding("unknown file kind")),
        })
    }

    fn value(&mut self) -> Result<observed::Value> {
        Ok(match self.byte()? {
            0 => observed::Value::Absent,
            1 => observed::Value::Error(self.u32()? as i32),
            2 => observed::Value::Metadata(observed::Metadata {
                kind: self.kind()?,
                device: self.u64()?,
                inode: self.u64()?,
                owner: self.u32()?,
                mode: self.u32()?,
                length: self.u64()?,
                modified_seconds: self.u64()? as i64,
                modified_nanos: self.u32()?,
                changed_seconds: self.u64()? as i64,
                changed_nanos: self.u32()?,
            }),
            3 => observed::Value::Bytes(self.bytes()?),
            4 => observed::Value::Link(self.string()?),
            5 => {
                let count = self.count()?;
                if count > self.remaining() / 17 {
                    return Err(Error::Encoding("impossible frontier count"));
                }
                let mut entries = Vec::new();
                self.budget.reserve(&mut entries, count)?;
                for _ in 0..count {
                    entries.push(observed::Entry {
                        name: self.string()?,
                        kind: self.kind()?,
                        inode: self.u64()?,
                    });
                }
                let end = match self.byte()? {
                    0 => observed::End::Eof,
                    1 => observed::End::Error(self.u32()? as i32),
                    _ => return Err(Error::Encoding("missing directory end")),
                };
                observed::Value::Directory { entries, end }
            }
            6 => observed::Value::Environment(None),
            7 => observed::Value::Environment(Some(self.string()?)),
            8 => observed::Value::Identity {
                effective_uid: self.u32()?,
            },
            9 => {
                let bytes = self.bytes()?;
                #[cfg(test)]
                super::OWNED_BYTES.with(|count| count.set(count.get() + 1));
                let end = match self.byte()? {
                    0 => observed::End::Eof,
                    1 => observed::End::Error(self.u32()? as i32),
                    _ => return Err(Error::Encoding("missing file end")),
                };
                observed::Value::File { bytes, end }
            }
            10 => observed::Value::EnvironmentPrefix(false),
            11 => observed::Value::EnvironmentPrefix(true),
            12 => observed::Value::Discovery(self.discovery()?),
            13 => observed::Value::Canonical(Ok(self.path()?)),
            14 => observed::Value::Canonical(Err(self.u32()? as i32)),
            15 => observed::Value::DirectoryStream(self.stream()?),
            16 => observed::Value::Traversal(self.traversal()?),
            17 => observed::Value::GitRead(self.git_read()?),
            _ => return Err(Error::Encoding("unknown observation tag")),
        })
    }

    fn boolean(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Encoding("invalid discovery boolean")),
        }
    }

    fn filename(&mut self) -> Result<Filename> {
        match self.byte()? {
            0 => Ok(Filename::Config),
            1 => Ok(Filename::Credentials),
            _ => Err(Error::Encoding("unknown discovery filename")),
        }
    }

    fn native_error(&mut self) -> Result<NativeError> {
        match self.byte()? {
            0 => Ok(NativeError::InvalidInput),
            1 => Ok(NativeError::Os(self.u32()? as i32)),
            _ => Err(Error::Encoding("unknown discovery native error")),
        }
    }

    fn attempt(&mut self) -> Result<Attempt> {
        match self.byte()? {
            0 => Ok(Attempt::Acquired(Identity {
                device: self.u64()?,
                inode: self.u64()?,
            })),
            1 => Ok(Attempt::OpenFailed(self.native_error()?)),
            2 => Ok(Attempt::StatFailed(self.u32()? as i32)),
            _ => Err(Error::Encoding("unknown discovery attempt")),
        }
    }

    fn discovery(&mut self) -> Result<Transcript> {
        let filename = self.filename()?;
        let warn = self.boolean()?;
        let legacy = self.attempt()?;
        let after = match self.byte()? {
            0 => After::None,
            1 => After::Second(self.attempt()?),
            2 => After::Fallback(match self.byte()? {
                0 => Exists::Present,
                1 => Exists::Failed(self.native_error()?),
                _ => return Err(Error::Encoding("unknown discovery exists result")),
            }),
            _ => return Err(Error::Encoding("unknown discovery branch")),
        };
        Transcript::checked(filename, warn, legacy, after)
            .ok_or(Error::Encoding("invalid discovery transcript"))
    }
}

impl Encoder<'_> {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.length = self
            .length
            .checked_add(bytes.len())
            .ok_or(Error::Encoding("encoded length overflow"))?;
        if let Some(output) = self.output.as_mut() {
            output.extend_from_slice(bytes);
        }
        Ok(())
    }

    fn byte(&mut self, byte: u8) -> Result<()> {
        self.append(&[byte])
    }

    fn u32(&mut self, value: u32) -> Result<()> {
        self.append(&value.to_le_bytes())
    }

    fn u64(&mut self, value: u64) -> Result<()> {
        self.append(&value.to_le_bytes())
    }

    fn count(&mut self, value: usize) -> Result<()> {
        self.u64(u64::try_from(value).map_err(|_| Error::Encoding("encoded count overflow"))?)
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.count(bytes.len())?;
        self.append(bytes)
    }

    fn image(&mut self, image: &Stored) -> Result<()> {
        self.append(MAGIC)?;
        self.append(CARGO_COMMIT.as_bytes())?;
        self.count(image.records.len())?;
        for record in &image.records {
            self.query(&record.query)?;
            self.value(&record.value)?;
        }
        Ok(())
    }

    fn query(&mut self, query: &observed::Query) -> Result<()> {
        match query {
            observed::Query::Metadata { path, follow } => {
                self.byte(u8::from(*follow))?;
                self.bytes(path.bytes())
            }
            observed::Query::Read {
                path,
                offset,
                length,
            } => {
                self.byte(2)?;
                self.bytes(path.bytes())?;
                self.u64(*offset)?;
                self.u64(*length)
            }
            observed::Query::Link(path) => {
                self.byte(3)?;
                self.bytes(path.bytes())
            }
            observed::Query::Directory(path) => {
                self.byte(4)?;
                self.bytes(path.bytes())
            }
            observed::Query::Environment(key) => {
                self.byte(5)?;
                self.bytes(key)
            }
            observed::Query::EnvironmentPrefix(key) => {
                self.byte(8)?;
                self.bytes(key)
            }
            observed::Query::Identity => self.byte(6),
            observed::Query::File(path) => {
                self.byte(7)?;
                self.bytes(path.bytes())
            }
            observed::Query::Canonical(path) => {
                self.byte(10)?;
                self.bytes(path.bytes())
            }
            observed::Query::DirectoryStream(path) => {
                self.byte(11)?;
                self.bytes(path.bytes())
            }
            observed::Query::Traversal {
                root,
                is_root,
                invocation,
            } => {
                self.byte(12)?;
                self.bytes(root.bytes())?;
                self.byte(u8::from(*is_root))?;
                self.u64(*invocation)
            }
            observed::Query::Discovery {
                directory,
                filename,
                warn,
            } => {
                self.byte(9)?;
                self.bytes(directory.bytes())?;
                self.filename(*filename)?;
                self.byte(u8::from(*warn))
            }
            observed::Query::GitRead { path, role } => {
                self.byte(13)?;
                self.bytes(path.bytes())?;
                self.git_role(*role)
            }
        }
    }

    fn kind(&mut self, kind: observed::Kind) -> Result<()> {
        self.byte(match kind {
            observed::Kind::File => 0,
            observed::Kind::Directory => 1,
            observed::Kind::Symlink => 2,
            observed::Kind::Fifo => 3,
            observed::Kind::Socket => 4,
            observed::Kind::Character => 5,
            observed::Kind::Block => 6,
            observed::Kind::Unknown => 7,
        })
    }

    fn value(&mut self, value: &observed::Value) -> Result<()> {
        match value {
            observed::Value::GitRead(transcript) => {
                self.byte(17)?;
                self.git_read(transcript)
            }
            observed::Value::Absent => self.byte(0),
            observed::Value::Error(code) => {
                self.byte(1)?;
                self.u32(*code as u32)
            }
            observed::Value::Metadata(value) => {
                self.byte(2)?;
                self.kind(value.kind)?;
                self.u64(value.device)?;
                self.u64(value.inode)?;
                self.u32(value.owner)?;
                self.u32(value.mode)?;
                self.u64(value.length)?;
                self.u64(value.modified_seconds as u64)?;
                self.u32(value.modified_nanos)?;
                self.u64(value.changed_seconds as u64)?;
                self.u32(value.changed_nanos)
            }
            observed::Value::Bytes(bytes) => {
                self.byte(3)?;
                self.bytes(bytes)
            }
            observed::Value::Link(bytes) => {
                self.byte(4)?;
                self.bytes(bytes)
            }
            observed::Value::Directory { entries, end } => {
                self.byte(5)?;
                self.count(entries.len())?;
                for entry in entries {
                    self.bytes(&entry.name)?;
                    self.kind(entry.kind)?;
                    self.u64(entry.inode)?;
                }
                match end {
                    observed::End::Eof => self.byte(0),
                    observed::End::Error(code) => {
                        self.byte(1)?;
                        self.u32(*code as u32)
                    }
                }
            }
            observed::Value::EnvironmentPrefix(false) => self.byte(10),
            observed::Value::EnvironmentPrefix(true) => self.byte(11),
            observed::Value::Environment(None) => self.byte(6),
            observed::Value::Environment(Some(bytes)) => {
                self.byte(7)?;
                self.bytes(bytes)
            }
            observed::Value::Identity { effective_uid } => {
                self.byte(8)?;
                self.u32(*effective_uid)
            }
            observed::Value::File { bytes, end } => {
                self.byte(9)?;
                self.bytes(bytes)?;
                match end {
                    observed::End::Eof => self.byte(0),
                    observed::End::Error(code) => {
                        self.byte(1)?;
                        self.u32(*code as u32)
                    }
                }
            }
            observed::Value::Discovery(transcript) => {
                self.byte(12)?;
                self.filename(transcript.filename())?;
                self.byte(u8::from(transcript.warn()))?;
                self.attempt(transcript.legacy())?;
                match transcript.after() {
                    After::None => self.byte(0),
                    After::Second(attempt) => {
                        self.byte(1)?;
                        self.attempt(attempt)
                    }
                    After::Fallback(exists) => {
                        self.byte(2)?;
                        match exists {
                            Exists::Present => self.byte(0),
                            Exists::Failed(error) => {
                                self.byte(1)?;
                                self.native_error(error)
                            }
                        }
                    }
                }
            }
            observed::Value::Canonical(Ok(path)) => {
                self.byte(13)?;
                self.bytes(path.bytes())
            }
            observed::Value::Canonical(Err(code)) => {
                self.byte(14)?;
                self.u32(*code as u32)
            }
            observed::Value::DirectoryStream(transcript) => {
                self.byte(15)?;
                self.stream(transcript)
            }
            observed::Value::Traversal(transcript) => {
                self.byte(16)?;
                self.traversal(transcript)
            }
        }
    }

    fn filename(&mut self, filename: Filename) -> Result<()> {
        self.byte(match filename {
            Filename::Config => 0,
            Filename::Credentials => 1,
        })
    }

    fn native_error(&mut self, error: NativeError) -> Result<()> {
        match error {
            NativeError::InvalidInput => self.byte(0),
            NativeError::Os(code) => {
                self.byte(1)?;
                self.u32(code as u32)
            }
        }
    }

    fn attempt(&mut self, attempt: Attempt) -> Result<()> {
        match attempt {
            Attempt::Acquired(identity) => {
                self.byte(0)?;
                self.u64(identity.device)?;
                self.u64(identity.inode)
            }
            Attempt::OpenFailed(error) => {
                self.byte(1)?;
                self.native_error(error)
            }
            Attempt::StatFailed(code) => {
                self.byte(2)?;
                self.u32(code as u32)
            }
        }
    }
}
