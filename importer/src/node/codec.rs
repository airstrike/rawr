use super::content::Edge;
use super::{ActionKey, Digest, Direct, Error, Kind, Value, STORAGE_SCHEMA};

const MAGIC: &[u8; 8] = b"RAWRNODE";
const HEADER_BYTES: usize = 116;
const PARENT_BYTES: usize = 104;
const TAIL_BYTES: usize = 48;
const MAX_PARENTS: usize = 65_536;
pub(crate) const MAX_ENVELOPE_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct Envelope {
    pub(crate) kind: Kind,
    pub(crate) output_schema: u32,
    pub(crate) action: ActionKey,
    pub(crate) recipe: Digest,
    pub(crate) context: Digest,
    parents: Box<[HistoricalParent]>,
    pub(crate) content: Digest,
    pub(crate) content_length: usize,
    pub(crate) output_limit: usize,
}

struct HistoricalParent {
    kind: Kind,
    schema: u32,
    action: ActionKey,
    receipt: Digest,
    content: Digest,
}

impl Envelope {
    pub(crate) fn reservation_length(parent_count: usize) -> Result<usize, Error> {
        encoded_length(parent_count)
    }

    pub(crate) fn direct<K: Value>(
        recipe: &Direct<K>,
        content: Digest,
        content_length: usize,
    ) -> Result<Self, Error> {
        let mut parents = Vec::new();
        parents
            .try_reserve_exact(recipe.parents.len())
            .map_err(|_| Error::Allocation)?;
        for parent in &recipe.parents {
            parents.push(HistoricalParent {
                kind: parent.kind(),
                schema: parent.schema(),
                action: parent.action(),
                receipt: parent.read_set().digest(),
                content: parent.content(),
            });
        }
        Ok(Self {
            kind: K::KIND,
            output_schema: K::SCHEMA,
            action: recipe.action,
            recipe: recipe.recipe,
            context: recipe.context,
            parents: parents.into_boxed_slice(),
            content,
            content_length,
            output_limit: recipe.output_limit,
        })
    }

    pub(crate) fn encode(&self) -> Result<Box<[u8]>, Error> {
        let length = encoded_length(self.parents.len())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&STORAGE_SCHEMA.to_le_bytes());
        bytes.push(self.kind as u8);
        bytes.push(0);
        bytes.extend_from_slice(&self.output_schema.to_le_bytes());
        bytes.extend_from_slice(&self.action.as_bytes());
        bytes.extend_from_slice(&self.recipe.as_bytes());
        bytes.extend_from_slice(&self.context.as_bytes());
        bytes.extend_from_slice(&(self.parents.len() as u32).to_le_bytes());
        for parent in &self.parents {
            bytes.push(parent.kind as u8);
            bytes.extend_from_slice(&[0; 3]);
            bytes.extend_from_slice(&parent.schema.to_le_bytes());
            bytes.extend_from_slice(&parent.action.as_bytes());
            bytes.extend_from_slice(&parent.receipt.as_bytes());
            bytes.extend_from_slice(&parent.content.as_bytes());
        }
        bytes.extend_from_slice(&self.content.as_bytes());
        bytes.extend_from_slice(&(self.content_length as u64).to_le_bytes());
        bytes.extend_from_slice(&(self.output_limit as u64).to_le_bytes());
        if bytes.len() != length {
            return Err(Error::State("node envelope encoder length drifted"));
        }
        Ok(bytes.into_boxed_slice())
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_BYTES.saturating_add(TAIL_BYTES) {
            return Err(Error::Codec("node envelope is truncated"));
        }
        if &bytes[..8] != MAGIC {
            return Err(Error::Codec("node envelope magic changed"));
        }
        if u16::from_le_bytes([bytes[8], bytes[9]]) != STORAGE_SCHEMA {
            return Err(Error::Codec("node envelope schema changed"));
        }
        if bytes[11] != 0 {
            return Err(Error::Codec("node envelope reserved field changed"));
        }
        let kind = Kind::from_code(bytes[10]).ok_or(Error::Codec("node kind is unknown"))?;
        let output_schema = u32_at(bytes, 12, "node output schema is truncated")?;
        let action = ActionKey::from_bytes(array_at(bytes, 16, "node action key is truncated")?);
        let recipe = Digest::from_bytes(array_at(bytes, 48, "node recipe digest is truncated")?);
        let context = Digest::from_bytes(array_at(bytes, 80, "node context digest is truncated")?);
        let parent_count = usize::try_from(u32_at(bytes, 112, "node parent count is truncated")?)
            .map_err(|_| Error::Codec("node parent count is not addressable"))?;
        let expected = encoded_length(parent_count)?;
        if bytes.len() != expected {
            return Err(Error::Codec("node envelope length changed"));
        }
        let mut parents = Vec::new();
        parents
            .try_reserve_exact(parent_count)
            .map_err(|_| Error::Allocation)?;
        let mut offset = HEADER_BYTES;
        for _ in 0..parent_count {
            let parent_kind = Kind::from_code(bytes[offset])
                .ok_or(Error::Codec("node parent kind is unknown"))?;
            if bytes[offset + 1..offset + 4] != [0; 3] {
                return Err(Error::Codec("node parent reserved field changed"));
            }
            parents.push(HistoricalParent {
                kind: parent_kind,
                schema: u32_at(bytes, offset + 4, "node parent schema is truncated")?,
                action: ActionKey::from_bytes(array_at(
                    bytes,
                    offset + 8,
                    "node parent action is truncated",
                )?),
                receipt: Digest::from_bytes(array_at(
                    bytes,
                    offset + 40,
                    "node parent receipt is truncated",
                )?),
                content: Digest::from_bytes(array_at(
                    bytes,
                    offset + 72,
                    "node parent content is truncated",
                )?),
            });
            offset = offset.saturating_add(PARENT_BYTES);
        }
        let content =
            Digest::from_bytes(array_at(bytes, offset, "node content digest is truncated")?);
        let content_length = usize::try_from(u64_at(
            bytes,
            offset + 32,
            "node content length is truncated",
        )?)
        .map_err(|_| Error::Codec("node content length is not addressable"))?;
        let output_limit = usize::try_from(u64_at(
            bytes,
            offset + 40,
            "node content limit is truncated",
        )?)
        .map_err(|_| Error::Codec("node content limit is not addressable"))?;
        if content_length > output_limit || output_limit > super::MAX_CONTENT_BYTES {
            return Err(Error::Codec("node content length exceeds its sealed limit"));
        }
        Ok(Self {
            kind,
            output_schema,
            action,
            recipe,
            context,
            parents: parents.into_boxed_slice(),
            content,
            content_length,
            output_limit,
        })
    }

    pub(crate) fn matches<K: Value>(&self, current: &Direct<K>) -> bool {
        self.kind == K::KIND
            && self.output_schema == K::SCHEMA
            && self.action == current.action
            && self.recipe == current.recipe
            && self.context == current.context
            && self.output_limit == current.output_limit
            && self.parents.len() == current.parents.len()
            && self
                .parents
                .iter()
                .zip(current.parents.iter())
                .all(|(historical, parent)| {
                    historical.kind == parent.kind()
                        && historical.schema == parent.schema()
                        && historical.content == parent.content()
                })
    }
}

fn encoded_length(parents: usize) -> Result<usize, Error> {
    if parents > MAX_PARENTS {
        return Err(Error::Limit {
            resource: "node envelope parents",
            limit: MAX_PARENTS,
            actual: parents,
        });
    }
    let length = parents
        .checked_mul(PARENT_BYTES)
        .and_then(|parents| HEADER_BYTES.checked_add(parents))
        .and_then(|length| length.checked_add(TAIL_BYTES))
        .unwrap_or(usize::MAX);
    if length > MAX_ENVELOPE_BYTES {
        return Err(Error::Limit {
            resource: "node envelope bytes",
            limit: MAX_ENVELOPE_BYTES,
            actual: length,
        });
    }
    Ok(length)
}

fn array_at(bytes: &[u8], offset: usize, error: &'static str) -> Result<[u8; 32], Error> {
    bytes
        .get(offset..offset.saturating_add(32))
        .ok_or(Error::Codec(error))?
        .try_into()
        .map_err(|_| Error::Codec(error))
}

fn u32_at(bytes: &[u8], offset: usize, error: &'static str) -> Result<u32, Error> {
    bytes
        .get(offset..offset.saturating_add(4))
        .ok_or(Error::Codec(error))?
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| Error::Codec(error))
}

fn u64_at(bytes: &[u8], offset: usize, error: &'static str) -> Result<u64, Error> {
    bytes
        .get(offset..offset.saturating_add(8))
        .ok_or(Error::Codec(error))?
        .try_into()
        .map(u64::from_le_bytes)
        .map_err(|_| Error::Codec(error))
}
