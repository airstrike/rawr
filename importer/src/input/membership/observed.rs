//! Typed observations retain negative probes, errors, and complete frontiers.

use super::{Budget, Error, Result};

pub(super) mod discovery;
pub(super) mod git_read;
pub(super) mod stream;
pub(super) mod traversal;

/// Native path bytes, before Cargo's own Unicode-validation point.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Path(Vec<u8>);

/// A closed filesystem query; no command, repository, or open handle is stored.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Query {
    Metadata {
        path: Path,
        follow: bool,
    },
    Read {
        path: Path,
        offset: u64,
        length: u64,
    },
    Link(Path),
    Directory(Path),
    /// Environment of the sealed Cargo command after env_clear and overrides.
    /// Its value is never supplied by ambient Rawr process environment lookup.
    Environment(Vec<u8>),
    Identity,
    /// Complete bytes from offset zero, with an explicit terminal outcome.
    /// Acquisition uses the consumer's original account, never a stored limit.
    File(Path),
    /// Whether any key in the sealed Cargo environment starts with these bytes.
    /// Callers include Cargo's trailing underscore; exact-key lookup is separate.
    EnvironmentPrefix(Vec<u8>),
    /// One closed config/credentials filename acquisition, not a live handle.
    Discovery {
        directory: Path,
        filename: discovery::Filename,
        warn: bool,
    },
    /// Exact native spelling passed to canonicalization, without freshness.
    Canonical(Path),
    /// Source-shaped ordered Cargo iterator, distinct from a sorted frontier.
    DirectoryStream(Path),
    /// One source-directed WalkDir invocation, distinct from complete readers.
    Traversal {
        root: Path,
        is_root: bool,
        invocation: u64,
    },
    GitRead {
        path: Path,
        role: git_read::Role,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    File,
    Directory,
    Symlink,
    Fifo,
    Socket,
    Character,
    Block,
    Unknown,
}

/// Identity, trust, and source metadata are separate from content bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Metadata {
    pub(super) kind: Kind,
    pub(super) device: u64,
    pub(super) inode: u64,
    pub(super) owner: u32,
    pub(super) mode: u32,
    pub(super) length: u64,
    pub(super) modified_seconds: i64,
    pub(super) modified_nanos: u32,
    pub(super) changed_seconds: i64,
    pub(super) changed_nanos: u32,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Entry {
    pub(super) name: Vec<u8>,
    pub(super) kind: Kind,
    pub(super) inode: u64,
}

/// Only a terminal EOF or explicit error closes a file or directory observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum End {
    Eof,
    Error(i32),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Value {
    Absent,
    Error(i32),
    Metadata(Metadata),
    Bytes(Vec<u8>),
    Link(Vec<u8>),
    Directory {
        entries: Vec<Entry>,
        end: End,
    },
    Environment(Option<Vec<u8>>),
    EnvironmentPrefix(bool),
    Identity {
        effective_uid: u32,
    },
    /// Capability-free complete-read data, not a current source receipt.
    /// Empty content requires EOF; failure before content uses `Error`.
    File {
        bytes: Vec<u8>,
        end: End,
    },
    Discovery(discovery::Transcript),
    /// Successful absolute native bytes or positive native errno. Missing,
    /// resource and malformed-native-result failures cannot enter this value.
    Canonical(std::result::Result<Path, i32>),
    DirectoryStream(stream::Transcript),
    Traversal(traversal::Transcript),
    GitRead(git_read::Transcript),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Record {
    pub(super) query: Query,
    pub(super) value: Value,
}

impl Path {
    pub(super) fn from_bytes(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Self> {
        budget
            .allocation
            .string_exceeded("membership native path bytes", bytes.len());
        if !bytes.starts_with(b"/") || bytes.contains(&0) {
            return Err(Error::Encoding("expected absolute native path"));
        }
        Ok(Self(budget.bytes(bytes)?))
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Record {
    pub(super) fn checked(self) -> Result<Self> {
        let valid = match (&self.query, &self.value) {
            (Query::Metadata { .. }, Value::Absent | Value::Error(_) | Value::Metadata(_))
            | (Query::Read { .. }, Value::Absent | Value::Error(_) | Value::Bytes(_))
            | (Query::Link(_), Value::Absent | Value::Error(_) | Value::Link(_))
            | (Query::Directory(_), Value::Absent | Value::Error(_) | Value::Directory { .. })
            | (Query::Environment(_), Value::Environment(_))
            | (Query::EnvironmentPrefix(_), Value::EnvironmentPrefix(_))
            | (Query::Identity, Value::Identity { .. })
            | (Query::Canonical(_), Value::Canonical(_))
            | (Query::DirectoryStream(_), Value::DirectoryStream(_))
            | (Query::Traversal { .. }, Value::Traversal(_))
            | (Query::File(_), Value::Absent | Value::Error(_) | Value::File { .. }) => true,
            (Query::Discovery { filename, warn, .. }, Value::Discovery(transcript)) => {
                transcript.filename() == *filename
                    && transcript.warn() == *warn
                    // Path already rejects NUL, and both fixed suffixes are
                    // NUL-free. The standalone transcript permits this branch
                    // for other callers; this observation cannot represent it.
                    && !matches!(transcript.legacy(), discovery::Attempt::OpenFailed(discovery::NativeError::InvalidInput))
            }
            (Query::GitRead { role, .. }, Value::GitRead(transcript)) => transcript.role() == *role,
            _ => false,
        };
        if !valid {
            return Err(Error::ObservationType);
        }
        match &self.query {
            Query::Environment(key) | Query::EnvironmentPrefix(key)
                if key.is_empty() || key.contains(&0) || key.contains(&b'=') =>
            {
                return Err(Error::Encoding("invalid environment key"));
            }
            Query::Read { length, offset, .. } => {
                offset
                    .checked_add(*length)
                    .ok_or(Error::Encoding("read range overflow"))?;
                if let Value::Bytes(bytes) = &self.value
                    && u64::try_from(bytes.len()).ok() != Some(*length)
                {
                    return Err(Error::Encoding("read range length mismatch"));
                }
            }
            _ => {}
        }
        match &self.value {
            Value::DirectoryStream(transcript) => transcript.validate()?,
            Value::Traversal(transcript) => transcript.validate()?,
            Value::GitRead(transcript) => transcript.validate()?,
            Value::Canonical(Err(code)) if *code <= 0 => {
                return Err(Error::Encoding(
                    "canonicalization native error must be positive",
                ));
            }
            Value::Metadata(metadata)
                if metadata.modified_nanos >= 1_000_000_000
                    || metadata.changed_nanos >= 1_000_000_000 =>
            {
                return Err(Error::Encoding("invalid metadata nanoseconds"));
            }
            Value::Error(0)
            | Value::Directory {
                end: End::Error(0), ..
            }
            | Value::File {
                end: End::Error(0), ..
            } => {
                return Err(Error::Encoding("zero is not an OS error"));
            }
            Value::File {
                bytes,
                end: End::Error(_),
            } if bytes.is_empty() => {
                return Err(Error::Encoding(
                    "file failure before content requires an error result",
                ));
            }
            Value::Link(bytes) if bytes.is_empty() || bytes.contains(&0) => {
                return Err(Error::Encoding("invalid symlink target"));
            }
            Value::Directory { entries, .. } => {
                for entry in entries {
                    if entry.name.is_empty()
                        || entry.name == b"."
                        || entry.name == b".."
                        || entry.name.contains(&0)
                        || entry.name.contains(&b'/')
                    {
                        return Err(Error::Encoding("invalid directory entry name"));
                    }
                }
                for pair in entries.windows(2) {
                    if pair[0].name >= pair[1].name {
                        return Err(Error::Encoding(
                            "directory frontier is not sorted and unique",
                        ));
                    }
                }
            }
            _ => {}
        }
        Ok(self)
    }
}
