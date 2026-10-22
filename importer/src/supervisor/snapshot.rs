//! Canonical bounded Rawr snapshot transport bound to sealed digest evidence.

use std::ops::Range;

use sha2::{Digest as _, Sha256};

/// Cumulative source validation ownership for one original reload attempt.
pub mod attempt;
mod freshness;

pub(crate) use freshness::Freshness;

const ENTRY_DOMAIN: &[u8] = b"rawr.snapshot.entry/v1";
const KEY_BYTES: usize = 64;

/// Owned exact snapshot frames whose digest equals one sealed snapshot.
pub struct Bytes {
    framed: Box<[u8]>,
    digest: [u8; 32],
}

/// Single-buffer canonical entry/file capture.
pub struct Capture {
    framed: Vec<u8>,
    entry: bool,
    files: usize,
    previous: Option<Range<usize>>,
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    account: crate::surface::account::Account,
}

impl Bytes {
    pub(crate) const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Decode already-framed bytes and bind them to exact snapshot evidence.
    pub fn decode(
        expected: crate::surface::Snapshot,
        framed: Box<[u8]>,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        let mut account = crate::surface::account::Account::supervisor();
        Self::decode_accounted(expected, framed, limits, &mut account)
    }

    fn decode_accounted(
        expected: crate::surface::Snapshot,
        framed: Box<[u8]>,
        limits: &crate::cargo::Limits,
        account: &mut crate::surface::account::Account,
    ) -> Result<Self, Error> {
        validate(&framed, limits, account)?;
        let digest: [u8; 32] = Sha256::digest(&framed).into();
        if digest != *expected.as_bytes() {
            return Err(Error::Mismatch);
        }
        Ok(Self { framed, digest })
    }

    /// Start one canonical capture using the same snapshot limits.
    #[must_use]
    pub fn capture(limits: &crate::cargo::Limits) -> Capture {
        Capture {
            framed: Vec::new(),
            entry: false,
            files: 0,
            previous: None,
            items: 0,
            bytes: 0,
            item_limit: limits.items,
            byte_limit: limits.json_bytes,
            string_limit: limits.string_bytes,
            account: crate::surface::account::Account::supervisor(),
        }
    }

    pub(crate) fn matches(&self, expected: crate::surface::Snapshot) -> bool {
        self.digest == *expected.as_bytes()
    }

    pub(crate) fn into_inner(self) -> Box<[u8]> {
        self.framed
    }
}

impl Capture {
    /// The accounting axes this capture crossed so far.
    #[doc(hidden)]
    #[must_use]
    pub const fn exceedances(&self) -> crate::exceedance::Record {
        self.account.exceedances()
    }

    /// Frame the exact application entry before any file.
    pub fn entry(&mut self, path: &str) -> Result<(), Error> {
        if self.entry || self.items != 0 {
            return Err(Error::Order);
        }
        self.check_path(path);
        let charge = self.charged(1, path.len())?;
        self.reserve(ENTRY_DOMAIN.len() + 8 + path.len())?;
        self.framed.extend_from_slice(ENTRY_DOMAIN);
        push_length(&mut self.framed, path.len())?;
        self.framed.extend_from_slice(path.as_bytes());
        (self.items, self.bytes) = charge;
        self.entry = true;
        Ok(())
    }

    /// Frame one canonically ordered file without cloning its contents.
    pub fn file(&mut self, path: &str, contents: &[u8]) -> Result<(), Error> {
        if !self.entry {
            return Err(Error::MissingEntry);
        }
        self.check_path(path);
        if self.previous.as_ref().is_some_and(|range| {
            self.framed
                .get(range.clone())
                .is_none_or(|previous| previous >= path.as_bytes())
        }) {
            return Err(Error::Order);
        }
        let retained = path.len().checked_add(contents.len()).ok_or(Error::Limit {
            resource: "snapshot retained bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        let charge = self.charged(1, retained)?;
        let additional = 8_usize
            .checked_add(path.len())
            .and_then(|bytes| bytes.checked_add(8))
            .and_then(|bytes| bytes.checked_add(contents.len()))
            .ok_or(Error::Oversized {
                limit: frame_limit(),
                actual: usize::MAX,
            })?;
        self.reserve(additional)?;
        push_length(&mut self.framed, path.len())?;
        let start = self.framed.len();
        self.framed.extend_from_slice(path.as_bytes());
        let end = self.framed.len();
        push_length(&mut self.framed, contents.len())?;
        self.framed.extend_from_slice(contents);
        (self.items, self.bytes) = charge;
        self.previous = Some(start..end);
        self.files = self.files.checked_add(1).ok_or(Error::Limit {
            resource: "snapshot source inputs",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        Ok(())
    }

    /// Move the sole retained buffer into transport proof if its digest is exact.
    pub fn bind(
        mut self,
        expected: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<Bytes, Error> {
        if !self.entry {
            return Err(Error::MissingEntry);
        }
        if self.files == 0 {
            return Err(Error::EmptySnapshot);
        }
        let framed = std::mem::take(&mut self.framed).into_boxed_slice();
        Bytes::decode_accounted(expected, framed, limits, &mut self.account)
    }

    /// The path length is an accounting target recorded on the
    /// supervisor account, never a refusal.
    fn check_path(&mut self, path: &str) {
        self.account
            .exceeded("snapshot path bytes", self.string_limit, path.len());
    }

    /// Charge items and bytes cumulatively. Crossing the item or byte
    /// target is recorded on the supervisor account and the charge proceeds;
    /// checked-arithmetic overflow stays terminal. The physical bound on the
    /// retained buffer is the IPC frame ceiling enforced by `reserve`.
    fn charged(&mut self, items: usize, bytes: usize) -> Result<(usize, usize), Error> {
        let items = self.items.checked_add(items).ok_or(Error::Limit {
            resource: "snapshot retained items",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        let bytes = self.bytes.checked_add(bytes).ok_or(Error::Limit {
            resource: "snapshot retained bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("snapshot retained items", self.item_limit, items);
        self.account
            .exceeded("snapshot retained bytes", self.byte_limit, bytes);
        Ok((items, bytes))
    }

    fn reserve(&mut self, additional: usize) -> Result<(), Error> {
        let actual = self
            .framed
            .len()
            .checked_add(additional)
            .ok_or(Error::Oversized {
                limit: frame_limit(),
                actual: usize::MAX,
            })?;
        if actual > frame_limit() {
            return Err(Error::Oversized {
                limit: frame_limit(),
                actual,
            });
        }
        self.framed
            .try_reserve_exact(additional)
            .map_err(|_| Error::Allocation)
    }
}

/// Prove the framing. The IPC frame ceiling is the physical bound and refuses;
/// the path, item and byte targets are accounting comparisons recorded on
/// `account` while validation continues over the whole transport.
fn validate(
    framed: &[u8],
    limits: &crate::cargo::Limits,
    account: &mut crate::surface::account::Account,
) -> Result<(), Error> {
    if framed.len() > frame_limit() {
        return Err(Error::Oversized {
            limit: frame_limit(),
            actual: framed.len(),
        });
    }
    let Some(mut cursor) = framed.strip_prefix(ENTRY_DOMAIN) else {
        return Err(Error::Malformed);
    };
    let entry = take_string(&mut cursor, limits.string_bytes, account)?;
    let mut items = 1_usize;
    let mut retained = entry.len();
    let mut files = 0_usize;
    let mut previous: Option<&str> = None;
    account.exceeded("snapshot retained bytes", limits.json_bytes, retained);
    while !cursor.is_empty() {
        let path = take_string(&mut cursor, limits.string_bytes, account)?;
        if previous.is_some_and(|prior| prior >= path) {
            return Err(Error::Order);
        }
        let contents = take_bytes(&mut cursor)?;
        items = items.checked_add(1).ok_or(Error::Malformed)?;
        retained = retained
            .checked_add(path.len())
            .and_then(|bytes| bytes.checked_add(contents.len()))
            .ok_or(Error::Malformed)?;
        account.exceeded("snapshot retained items", limits.items, items);
        account.exceeded("snapshot retained bytes", limits.json_bytes, retained);
        previous = Some(path);
        files = files.checked_add(1).ok_or(Error::Malformed)?;
    }
    if files == 0 {
        return Err(Error::EmptySnapshot);
    }
    Ok(())
}

fn take_string<'a>(
    cursor: &mut &'a [u8],
    string_limit: usize,
    account: &mut crate::surface::account::Account,
) -> Result<&'a str, Error> {
    let bytes = take_bytes(cursor)?;
    account.exceeded("snapshot path bytes", string_limit, bytes.len());
    std::str::from_utf8(bytes).map_err(|_| Error::Malformed)
}

/// One exact framed string of an already-validated transport.
pub(super) fn take_string_field<'a>(cursor: &mut &'a [u8]) -> Result<&'a str, Error> {
    std::str::from_utf8(take_bytes(cursor)?).map_err(|_| Error::Malformed)
}

/// A framed length beyond the buffer is malformed; the buffer itself is already
/// within the physical frame ceiling, so no further size bound applies here.
pub(super) fn take_bytes<'a>(cursor: &mut &'a [u8]) -> Result<&'a [u8], Error> {
    let length = take_length(cursor)?;
    let (value, remaining) = cursor.split_at_checked(length).ok_or(Error::Malformed)?;
    *cursor = remaining;
    Ok(value)
}

fn take_length(cursor: &mut &[u8]) -> Result<usize, Error> {
    let (length, remaining) = cursor.split_at_checked(8).ok_or(Error::Malformed)?;
    *cursor = remaining;
    let length = u64::from_le_bytes(length.try_into().map_err(|_| Error::Malformed)?);
    usize::try_from(length).map_err(|_| Error::Malformed)
}

fn push_length(bytes: &mut Vec<u8>, length: usize) -> Result<(), Error> {
    let length = u64::try_from(length).map_err(|_| Error::Malformed)?;
    bytes.extend_from_slice(&length.to_le_bytes());
    Ok(())
}

const fn frame_limit() -> usize {
    crate::protocol::FRAME_BYTES - KEY_BYTES
}

/// Canonical snapshot framing, limit, or equality failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("snapshot transport exceeded limit {limit} with observed size {actual}")]
    Oversized { limit: usize, actual: usize },
    #[error("snapshot transport has no application entry")]
    MissingEntry,
    #[error("snapshot transport has no source files")]
    EmptySnapshot,
    #[error("snapshot transport paths are duplicated or not canonical")]
    Order,
    #[error("snapshot transport is malformed")]
    Malformed,
    #[error("snapshot transport does not match the sealed request")]
    Mismatch,
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("cannot allocate canonical snapshot transport")]
    Allocation,
}

#[cfg(test)]
mod tests {
    use super::Bytes;

    const PATH: &str = "/fixture/main.rawr";
    const CONTENTS: &[u8] = b"fn main() {}\n";

    fn snapshot(limits: &crate::cargo::Limits) -> crate::surface::Snapshot {
        let mut digest = crate::surface::Snapshot::builder(limits);
        digest.entry(PATH).unwrap();
        digest.file(PATH, CONTENTS).unwrap();
        digest.seal().unwrap()
    }

    fn capture(limits: &crate::cargo::Limits) -> super::Capture {
        let mut capture = Bytes::capture(limits);
        capture.entry(PATH).unwrap();
        capture.file(PATH, CONTENTS).unwrap();
        capture
    }

    /// A snapshot whose retained bytes exceed the configured JSON target
    /// (the Gallery hot-reload case between 32 MiB + 1 and 256 MiB, modelled
    /// with a small configured target) captures, binds and decodes whole,
    /// recording the axis with its truthful cumulative figure; exactly at the
    /// target admits silently. The physical bound stays the IPC frame ceiling.
    #[test]
    fn snapshot_transport_past_its_targets_binds_and_records() {
        let retained = PATH.len() + PATH.len() + CONTENTS.len();
        let standard = crate::cargo::Limits::standard();
        let expected = snapshot(&standard);

        let mut exact = standard;
        exact.json_bytes = retained;
        exact.items = 2;
        let capture_exact = capture(&exact);
        assert!(!capture_exact.exceedances().is_exceeded());
        let bound = capture_exact
            .bind(expected, &exact)
            .expect("exact targets bind");

        let mut small = standard;
        small.json_bytes = retained - 1;
        small.items = 1;
        let past = capture(&small);
        let record = past.exceedances();
        let bytes = *record
            .axis("snapshot retained bytes")
            .expect("the byte target records its axis");
        assert_eq!(
            (bytes.target, bytes.first_requested, bytes.high_water),
            ((retained - 1) as u64, retained as u64, retained as u64)
        );
        let items = *record
            .axis("snapshot retained items")
            .expect("the item target records its axis");
        assert_eq!(
            (items.target, items.first_requested, items.high_water),
            (1, 2, 2)
        );
        let past = past
            .bind(expected, &small)
            .expect("a transport past its targets binds");
        assert_eq!(past.digest(), bound.digest());
        let decoded = Bytes::decode(expected, past.into_inner(), &small)
            .expect("already-framed bytes past the targets decode");
        assert!(decoded.matches(expected));

        let mut strings = standard;
        strings.string_bytes = PATH.len() - 1;
        let paths = capture(&strings);
        let axis = *paths
            .exceedances()
            .axis("snapshot path bytes")
            .expect("the path target records its axis");
        assert_eq!(
            (axis.target, axis.high_water),
            ((PATH.len() - 1) as u64, PATH.len() as u64)
        );
        paths.bind(expected, &strings).expect("long paths bind");

        // A terminal cause is still terminal: a digest mismatch refuses.
        let other = {
            let mut digest = crate::surface::Snapshot::builder(&standard);
            digest.entry(PATH).unwrap();
            digest.file(PATH, b"other").unwrap();
            digest.seal().unwrap()
        };
        assert!(matches!(
            capture(&small).bind(other, &small),
            Err(super::Error::Mismatch)
        ));
    }
}
