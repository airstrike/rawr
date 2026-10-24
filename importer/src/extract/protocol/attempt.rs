//! Borrowed completion transport; decoding grants no candidate-file authority.
//!
//! Callers own prospective I/O, allocation and hashing admission. This codec
//! neither allocates nor performs filesystem operations.

use std::io::{self, Write};

const MAGIC: &[u8; 8] = b"RAWRA002";
pub(crate) const MAX_PATH_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_ATTEMPTS: usize = 65_536;
pub(crate) const MAX_INVOCATIONS: usize = 4096;
pub(crate) const MAX_WIRE_BYTES: usize =
    85 + MAX_INVOCATIONS * 128 + MAX_ATTEMPTS * 4 + MAX_PATH_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Invocation<'a> {
    pub(crate) nonce: &'a str,
    pub(crate) correlation: &'a str,
}

pub(crate) struct Completion<'a> {
    pub(crate) arguments_digest: &'a str,
    pub(crate) conservative: bool,
    invocations: &'a [u8],
    paths: &'a [u8],
    queries: &'a [u8],
}

impl<'a> Completion<'a> {
    /// Compare with the parent's independently validated probe identity. This
    /// still grants no authority to interpret display strings as native paths.
    pub(crate) fn require_invocation<'b>(
        &self,
        arguments_digest: &str,
        invocations: impl IntoIterator<Item = Invocation<'b>>,
    ) -> Result<(), &'static str> {
        let mut expected = invocations.into_iter();
        let rows_match = self.invocations().all(|row| {
            expected.next().is_some_and(|expected| {
                row.nonce == expected.nonce && row.correlation == expected.correlation
            })
        }) && expected.next().is_none();
        if self.arguments_digest != arguments_digest || !rows_match {
            return Err("attempt completion belongs to a different invocation");
        }
        Ok(())
    }

    pub(crate) fn decode(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() > MAX_WIRE_BYTES {
            return Err("attempt completion exceeds wire limit");
        }
        let mut cursor = bytes;
        if take(&mut cursor, 8)? != MAGIC {
            return Err("unsupported attempt completion version");
        }
        let arguments_digest = identity(take(&mut cursor, 64)?)?;
        let conservative = match take(&mut cursor, 1)?[0] {
            0 => false,
            1 => true,
            _ => return Err("invalid attempt completion flags"),
        };
        let count = number(&mut cursor)?;
        if count == 0 || count > MAX_INVOCATIONS {
            return Err("invalid attempt invocation count");
        }
        let invocations = take(&mut cursor, count * 128)?;
        for row in invocations.as_chunks::<128>().0 {
            identity(&row[..64])?;
            identity(&row[64..])?;
        }
        let count = number(&mut cursor)?;
        if count > MAX_ATTEMPTS {
            return Err("attempt count exceeds limit");
        }
        let mut total = 0usize;
        let paths = population(&mut cursor, count, &mut total)?;
        let query_count = number(&mut cursor)?;
        if query_count > MAX_ATTEMPTS - count {
            return Err("combined attempt and query count exceeds limit");
        }
        let queries = population(&mut cursor, query_count, &mut total)?;
        if !cursor.is_empty() {
            return Err("trailing attempt completion bytes");
        }
        Ok(Self {
            arguments_digest,
            conservative,
            invocations,
            paths,
            queries,
        })
    }

    pub(crate) fn invocations(&self) -> impl ExactSizeIterator<Item = Invocation<'a>> {
        self.invocations
            .as_chunks::<128>()
            .0
            .iter()
            .map(|row| Invocation {
                nonce: std::str::from_utf8(&row[..64]).expect("validated identity"),
                correlation: std::str::from_utf8(&row[64..]).expect("validated identity"),
            })
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &'a str> {
        path_rows(self.paths)
    }

    pub(crate) fn queries(&self) -> impl Iterator<Item = &'a str> {
        path_rows(self.queries)
    }
}

fn population<'a>(
    cursor: &mut &'a [u8],
    count: usize,
    total: &mut usize,
) -> Result<&'a [u8], &'static str> {
    let start = *cursor;
    for _ in 0..count {
        let length = number(cursor)?;
        *total = total
            .checked_add(length)
            .ok_or("attempt path length overflow")?;
        if length == 0 || *total > MAX_PATH_BYTES {
            return Err("invalid attempt path population");
        }
        std::str::from_utf8(take(cursor, length)?).map_err(|_| "attempt path is not UTF-8")?;
    }
    Ok(&start[..start.len() - cursor.len()])
}

fn path_rows(mut cursor: &[u8]) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if cursor.is_empty() {
            return None;
        }
        let length = number(&mut cursor).expect("validated path framing");
        Some(
            std::str::from_utf8(take(&mut cursor, length).expect("validated path length"))
                .expect("validated path UTF-8"),
        )
    })
}

/// Validate the complete borrowed population before writing any bytes. Caller
/// must publish atomically only after this returns successfully.
#[allow(dead_code, reason = "only the driver binary encodes")]
pub(crate) fn encode<'a>(
    writer: &mut impl Write,
    arguments_digest: &str,
    conservative: bool,
    invocations: &[Invocation<'_>],
    paths: impl ExactSizeIterator<Item = &'a str> + Clone,
    queries: impl ExactSizeIterator<Item = &'a str> + Clone,
) -> io::Result<()> {
    let invalid = |reason| io::Error::new(io::ErrorKind::InvalidInput, reason);
    identity(arguments_digest.as_bytes()).map_err(invalid)?;
    if invocations.is_empty() || invocations.len() > MAX_INVOCATIONS {
        return Err(invalid("invalid attempt invocation count"));
    }
    for row in invocations {
        identity(row.nonce.as_bytes()).map_err(invalid)?;
        identity(row.correlation.as_bytes()).map_err(invalid)?;
    }
    if paths.len() > MAX_ATTEMPTS || queries.len() > MAX_ATTEMPTS - paths.len() {
        return Err(invalid("attempt count exceeds limit"));
    }
    let mut total = 0usize;
    for path in paths.clone().chain(queries.clone()) {
        total = total
            .checked_add(path.len())
            .ok_or_else(|| invalid("attempt path length overflow"))?;
        if path.is_empty() || total > MAX_PATH_BYTES {
            return Err(invalid("invalid attempt path population"));
        }
    }
    writer.write_all(MAGIC)?;
    writer.write_all(arguments_digest.as_bytes())?;
    writer.write_all(&[u8::from(conservative)])?;
    writer.write_all(&(invocations.len() as u32).to_le_bytes())?;
    for row in invocations {
        writer.write_all(row.nonce.as_bytes())?;
        writer.write_all(row.correlation.as_bytes())?;
    }
    writer.write_all(&(paths.len() as u32).to_le_bytes())?;
    for path in paths {
        writer.write_all(&(path.len() as u32).to_le_bytes())?;
        writer.write_all(path.as_bytes())?;
    }
    writer.write_all(&(queries.len() as u32).to_le_bytes())?;
    for path in queries {
        writer.write_all(&(path.len() as u32).to_le_bytes())?;
        writer.write_all(path.as_bytes())?;
    }
    Ok(())
}

pub(crate) fn identity(bytes: &[u8]) -> Result<&str, &'static str> {
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err("invalid attempt identity");
    }
    std::str::from_utf8(bytes).map_err(|_| "invalid attempt identity")
}

fn take<'a>(cursor: &mut &'a [u8], count: usize) -> Result<&'a [u8], &'static str> {
    if count > cursor.len() {
        return Err("truncated attempt completion");
    }
    let (head, tail) = cursor.split_at(count);
    *cursor = tail;
    Ok(head)
}

fn number(cursor: &mut &[u8]) -> Result<usize, &'static str> {
    let bytes = take(cursor, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn wire(paths: &[&str], conservative: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        encode(
            &mut bytes,
            ID,
            conservative,
            &[Invocation {
                nonce: ID,
                correlation: ID,
            }],
            paths.iter().copied(),
            ["/query\n雪.rmeta", "/query\n雪.rmeta"].into_iter(),
        )
        .unwrap();
        bytes
    }

    #[test]
    fn invocation_binding_requires_exact_digest_population_and_order() {
        let other = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let rows = [
            Invocation {
                nonce: ID,
                correlation: ID,
            },
            Invocation {
                nonce: other,
                correlation: ID,
            },
        ];
        let mut bytes = Vec::new();
        encode(&mut bytes, ID, false, &rows, [].into_iter(), [].into_iter()).unwrap();
        let completion = Completion::decode(&bytes).unwrap();
        assert_eq!(completion.paths().count(), 0);
        assert_eq!(completion.queries().count(), 0);
        completion.require_invocation(ID, rows).unwrap();
        assert!(completion.require_invocation(other, rows).is_err());
        assert!(completion
            .require_invocation(ID, rows[..1].iter().copied())
            .is_err());
        assert!(completion
            .require_invocation(ID, rows.into_iter().rev())
            .is_err());
        assert!(completion
            .require_invocation(ID, rows.into_iter().chain(rows))
            .is_err());
        let mut changed = rows;
        changed[1].correlation = other;
        assert!(completion.require_invocation(ID, changed).is_err());
    }

    #[test]
    fn preserves_zero_repeated_and_unusual_paths() {
        for paths in [vec![], vec!["/a\n雪.rlib", "/a\n雪.rlib"]] {
            for conservative in [false, true] {
                let bytes = wire(&paths, conservative);
                let decoded = Completion::decode(&bytes).unwrap();
                assert_eq!(decoded.arguments_digest, ID);
                assert_eq!(decoded.conservative, conservative);
                assert_eq!(
                    decoded.invocations().collect::<Vec<_>>(),
                    [Invocation {
                        nonce: ID,
                        correlation: ID
                    }]
                );
                assert_eq!(decoded.paths().collect::<Vec<_>>(), paths);
                assert_eq!(
                    decoded.queries().collect::<Vec<_>>(),
                    ["/query\n雪.rmeta", "/query\n雪.rmeta"]
                );
            }
        }
    }

    #[test]
    fn rejects_every_truncation_and_trailing_data() {
        let mut bytes = wire(&["/a.rlib"], false);
        for length in 0..bytes.len() {
            assert!(Completion::decode(&bytes[..length]).is_err());
        }
        bytes.push(0);
        assert!(Completion::decode(&bytes).is_err());
        let mut old = wire(&[], false);
        old[..8].copy_from_slice(b"RAWRA001");
        assert!(Completion::decode(&old).is_err());
    }

    #[test]
    fn both_populations_share_count_and_byte_limits() {
        let row = [Invocation {
            nonce: ID,
            correlation: ID,
        }];
        let attempts = vec!["a"; MAX_ATTEMPTS - 1];
        let mut bytes = Vec::new();
        encode(
            &mut bytes,
            ID,
            false,
            &row,
            attempts.iter().copied(),
            ["b"].into_iter(),
        )
        .unwrap();
        let decoded = Completion::decode(&bytes).unwrap();
        assert_eq!(decoded.paths().count(), MAX_ATTEMPTS - 1);
        assert_eq!(decoded.queries().collect::<Vec<_>>(), ["b"]);
        let query_count = 209 + attempts.len() * 5;
        let mut excessive = bytes.clone();
        excessive[query_count..query_count + 4].copy_from_slice(&2u32.to_le_bytes());
        assert!(Completion::decode(&excessive).is_err());
        bytes.clear();
        assert!(encode(
            &mut bytes,
            ID,
            false,
            &row,
            attempts.iter().copied(),
            ["b", "c"].into_iter()
        )
        .is_err());
        assert!(bytes.is_empty());

        let path = "x".repeat(MAX_PATH_BYTES - 1);
        encode(
            &mut bytes,
            ID,
            true,
            &row,
            [path.as_str()].into_iter(),
            ["y"].into_iter(),
        )
        .unwrap();
        assert_eq!(
            Completion::decode(&bytes).unwrap().queries().next(),
            Some("y")
        );
        let query_length = bytes.len() - 5;
        bytes[query_length..query_length + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes.push(b'z');
        assert!(Completion::decode(&bytes).is_err());
    }

    #[test]
    fn rejects_invalid_framing_identities_and_utf8() {
        let bytes = wire(&["/a.rlib"], false);
        for (offset, replacement) in [
            (0, b'X'),
            (8, b'G'),
            (72, 2),
            (73, 0),
            (77, b'G'),
            (141, b'G'),
            (209, 255),
            (213, 255),
        ] {
            let mut invalid = bytes.clone();
            invalid[offset] = replacement;
            assert!(Completion::decode(&invalid).is_err(), "offset {offset}");
        }
        let mut query = wire(&[], false);
        query[217] = 255;
        assert!(Completion::decode(&query).is_err());
        let mut query = wire(&[], false);
        query[213..217].copy_from_slice(&0u32.to_le_bytes());
        assert!(Completion::decode(&query).is_err());
    }

    #[test]
    fn validates_population_before_writing() {
        let mut bytes = Vec::new();
        assert!(encode(&mut bytes, ID, false, &[], [].into_iter(), [].into_iter()).is_err());
        assert!(bytes.is_empty());
        let row = [Invocation {
            nonce: ID,
            correlation: ID,
        }];
        let path = "x".repeat(MAX_PATH_BYTES);
        assert!(encode(
            &mut bytes,
            ID,
            false,
            &row,
            [path.as_str()].into_iter(),
            ["x"].into_iter()
        )
        .is_err());
        assert!(bytes.is_empty());
        encode(
            &mut bytes,
            ID,
            false,
            &row,
            [path.as_str()].into_iter(),
            [].into_iter(),
        )
        .unwrap();
        assert_eq!(
            Completion::decode(&bytes).unwrap().paths().next(),
            Some(path.as_str())
        );
    }
}
