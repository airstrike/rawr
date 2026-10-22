//! Memo of file digests keyed by physical file identity, kept across launches.
//!
//! A digest is a pure function of a file's bytes. The same sealed store
//! payload, runner-bundle member or freshness input is checked by many
//! consumers, in this process and in every later launch; each check used to
//! re-read and re-hash the whole file. This memo computes each digest once per
//! observed identity and lets every later consumer reuse it.
//!
//! The identity is the file's device, inode, length, modification time and
//! status-change time (both with nanoseconds). Any write, truncation, rename
//! over, permission or timestamp change alters at least one of them (the
//! status-change time cannot be set by user space), so a changed file never
//! matches a stale entry and is read and hashed again. Callers insert only
//! after confirming the identity was unchanged across their read.
//!
//! Once a store is attached, every new digest is also appended to a table of
//! fixed-size checked records beside it, and the table is loaded on attach, so
//! an unchanged file is not read again by a later process. A torn, corrupt or
//! full table is emptied; correctness never depends on a hit.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use sha2::{Digest as _, Sha256};

/// Physical identity of one regular file at one observation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Identity {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Identity {
    /// The identity of a regular file from `std` metadata.
    #[cfg(unix)]
    pub(crate) fn of(metadata: &std::fs::Metadata) -> Option<Self> {
        use std::os::unix::fs::MetadataExt as _;
        metadata.is_file().then(|| Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.size(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }

    /// The file's length in bytes.
    pub(crate) fn length(self) -> u64 {
        self.length
    }

    #[cfg(not(unix))]
    pub(crate) fn of(_metadata: &std::fs::Metadata) -> Option<Self> {
        None
    }

    /// The identity of a regular file from a raw `fstat`.
    #[cfg(unix)]
    pub(crate) fn of_stat(stat: &nix::sys::stat::FileStat) -> Option<Self> {
        let kind = u32::from(stat.st_mode) & u32::from(nix::libc::S_IFMT);
        (kind == u32::from(nix::libc::S_IFREG)).then_some(Self {
            device: stat.st_dev as u64,
            inode: stat.st_ino,
            length: stat.st_size as u64,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        })
    }
}

/// Which pure function of the bytes the memoized digest is.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Domain {
    /// Plain SHA-256 of the file bytes.
    Sha256,
    /// `node::content_digest_parts(kind, schema, bytes)`.
    Content { kind: u8, schema: u32 },
}

/// Bound on remembered digests (about 100 bytes each). Reaching it clears the
/// memo and its table; correctness never depends on a hit.
const CAPACITY: usize = 1 << 18;

/// Identity (7 words), domain (tag, kind, schema), digest, 8-byte check.
const RECORD: usize = 56 + 6 + 32 + 8;

#[derive(Default)]
struct Memo {
    digests: HashMap<(Identity, Domain), [u8; 32]>,
    table: Option<File>,
}

fn memo() -> &'static Mutex<Memo> {
    static MEMO: OnceLock<Mutex<Memo>> = OnceLock::new();
    MEMO.get_or_init(Mutex::default)
}

fn record(identity: Identity, domain: Domain, digest: [u8; 32]) -> [u8; RECORD] {
    let mut record = [0_u8; RECORD];
    let words = [
        identity.device,
        identity.inode,
        identity.length,
        identity.modified.0 as u64,
        identity.modified.1 as u64,
        identity.changed.0 as u64,
        identity.changed.1 as u64,
    ];
    for (index, word) in words.into_iter().enumerate() {
        record[index * 8..index * 8 + 8].copy_from_slice(&word.to_le_bytes());
    }
    let (tag, kind, schema) = match domain {
        Domain::Sha256 => (0, 0, 0),
        Domain::Content { kind, schema } => (1, kind, schema),
    };
    record[56] = tag;
    record[57] = kind;
    record[58..62].copy_from_slice(&schema.to_le_bytes());
    record[62..94].copy_from_slice(&digest);
    let check = Sha256::digest(&record[..94]);
    record[94..].copy_from_slice(&check[..8]);
    record
}

fn parse(record: &[u8; RECORD]) -> Option<((Identity, Domain), [u8; 32])> {
    if Sha256::digest(&record[..94])[..8] != record[94..] {
        return None;
    }
    let word = |index: usize| {
        u64::from_le_bytes(
            record[index * 8..index * 8 + 8]
                .try_into()
                .unwrap_or_default(),
        )
    };
    let identity = Identity {
        device: word(0),
        inode: word(1),
        length: word(2),
        modified: (word(3) as i64, word(4) as i64),
        changed: (word(5) as i64, word(6) as i64),
    };
    let schema = u32::from_le_bytes(record[58..62].try_into().ok()?);
    let domain = match record[56] {
        0 => Domain::Sha256,
        1 => Domain::Content {
            kind: record[57],
            schema,
        },
        _ => return None,
    };
    Some(((identity, domain), record[62..94].try_into().ok()?))
}

/// Load the digests table at `path` and append later digests to it. The first
/// attach in a process wins; failures leave the memo process-local.
pub(crate) fn attach(path: &Path) {
    let Ok(mut memo) = memo().lock() else { return };
    if memo.table.is_some() {
        return;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::libc::O_NOFOLLOW);
    }
    let Ok(mut table) = options.open(path) else {
        return;
    };
    match load(&mut table) {
        Some(records) => memo.digests.extend(records),
        None => drop(table.set_len(0)),
    }
    memo.table = Some(table);
}

/// Every record of a whole, checked table that fits the memo.
fn load(table: &mut File) -> Option<Vec<((Identity, Domain), [u8; 32])>> {
    let mut bytes = Vec::new();
    table.read_to_end(&mut bytes).ok()?;
    if bytes.len() % RECORD != 0 || bytes.len() / RECORD >= CAPACITY {
        return None;
    }
    bytes.as_chunks::<RECORD>().0.iter().map(parse).collect()
}

/// The digest previously computed for exactly this identity, if any.
pub(crate) fn get(identity: Option<Identity>, domain: Domain) -> Option<[u8; 32]> {
    let identity = identity?;
    memo()
        .lock()
        .ok()?
        .digests
        .get(&(identity, domain))
        .copied()
}

/// Remember a digest computed from bytes read while the file kept `identity`.
pub(crate) fn insert(identity: Option<Identity>, domain: Domain, digest: [u8; 32]) {
    let Some(identity) = identity else { return };
    let Ok(mut memo) = memo().lock() else { return };
    let memo = &mut *memo;
    if memo.digests.len() >= CAPACITY {
        memo.digests.clear();
        if let Some(table) = &memo.table {
            drop(table.set_len(0));
        }
    }
    if memo.digests.insert((identity, domain), digest) != Some(digest)
        && let Some(table) = &mut memo.table
    {
        // One append-mode write per record keeps concurrent writers whole.
        drop(table.write_all(&record(identity, domain, digest)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_file_never_matches_its_previous_identity() {
        let directory = std::env::temp_dir().join(format!(
            "rawr-file-digest-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("temporary directory");
        let path = directory.join("member");
        std::fs::write(&path, b"original").expect("file is written");
        let before = Identity::of(&std::fs::metadata(&path).expect("metadata"));
        insert(before, Domain::Sha256, [7; 32]);
        assert_eq!(get(before, Domain::Sha256), Some([7; 32]));
        assert_eq!(get(before, Domain::Content { kind: 1, schema: 1 }), None);

        // Same length, rewritten in place: the status-change time moves even
        // when the modification time is restored.
        let modified = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("file opens");
        file.write_all(b"replaced").expect("file is rewritten");
        file.set_modified(modified).expect("mtime is restored");
        drop(file);
        let after = Identity::of(&std::fs::metadata(&path).expect("metadata"));
        assert_ne!(before, after);
        assert_eq!(get(after, Domain::Sha256), None);
        assert_eq!(get(None, Domain::Sha256), None);
        std::fs::remove_dir_all(&directory).expect("temporary directory is removed");
    }

    #[test]
    fn a_later_process_reloads_digests_and_rejects_a_torn_table() {
        let directory =
            std::env::temp_dir().join(format!("rawr-digest-table-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temporary directory");
        let member = directory.join("member");
        std::fs::write(&member, b"bytes").expect("file is written");
        let identity = Identity::of(&std::fs::metadata(&member).expect("metadata"))
            .expect("a regular file has an identity");
        let content = Domain::Content { kind: 3, schema: 9 };
        let path = directory.join("digests");
        let mut written = record(identity, Domain::Sha256, [1; 32]).to_vec();
        written.extend(record(identity, content, [2; 32]));
        std::fs::write(&path, &written).expect("table is written");
        let reload = || load(&mut File::open(&path).expect("table opens"));
        let loaded: HashMap<_, _> = reload().expect("a whole table loads").into_iter().collect();
        assert_eq!(loaded.get(&(identity, Domain::Sha256)), Some(&[1; 32]));
        assert_eq!(loaded.get(&(identity, content)), Some(&[2; 32]));

        // A rewritten file has a new identity: the reloaded entry cannot match it.
        std::fs::write(&member, b"other").expect("file is rewritten");
        let changed = Identity::of(&std::fs::metadata(&member).expect("metadata"));
        assert!(changed.is_some_and(|changed| !loaded.contains_key(&(changed, Domain::Sha256))));

        // A torn append or a flipped bit discards the table instead of trusting it.
        written[70] ^= 1;
        std::fs::write(&path, &written).expect("table is corrupted");
        assert!(reload().is_none());
        written[70] ^= 1;
        written.extend_from_slice(&[0; RECORD / 2]);
        std::fs::write(&path, &written).expect("table is torn");
        assert!(reload().is_none());
        std::fs::remove_dir_all(&directory).expect("temporary directory is removed");
    }
}
