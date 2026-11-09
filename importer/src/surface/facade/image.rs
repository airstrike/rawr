//! A runner's plan as a binary image its registry reads where it lies
//! (#1518): each table sorted as the registry searches it and each index
//! the registry asks of a facade precomputed, so a runner's start parses,
//! sorts and hashes nothing, and builds an entry only when a program looks
//! it up. The bodies of types and items are stored once for every facade
//! (#1517) and parsed one at a time.
//!
//! Every number is a little-endian `u32`. A table is an offset and a count
//! of fixed-size records of words; a blob is an offset and a length of
//! bytes. The header and each facade's record are such words.

mod view;
mod write;

pub use view::{Alias, Facade, Group, Identities, Kind};

/// The first bytes of a runner image.
const MAGIC: &[u8; 8] = b"RAWRIMG1";

/// The header's words after the magic: the schema, the facade count, then
/// the tables of type bodies, item bodies, facades and the build order.
const HEADER_WORDS: usize = 11;

/// Each field of a facade's record, as two words.
#[derive(Clone, Copy)]
enum Field {
    Root,
    Digest,
    Modules,
    Types,
    Groups,
    Aliases,
    TraitHeaders,
    Impls,
    ImplsByTrait,
    Derives,
    Identities,
    Selectors,
    Structural,
    Names,
    Through,
    Macros,
    Reexports,
    TypeAliases,
}

const FIELDS: usize = 18;

/// The words of one record of each facade table.
mod stride {
    pub(super) const BLOB: usize = 2;
    pub(super) const TYPE: usize = 3;
    pub(super) const GROUP: usize = 6;
    pub(super) const ALIAS: usize = 5;
    pub(super) const HEADER: usize = 4;
    /// The first four bytes of a 32-byte key, then a value: each use of
    /// the table checks the whole key on what the value names.
    pub(super) const KEYED: usize = 2;
    /// A name, as a blob, then a value.
    pub(super) const NAME: usize = 3;
    /// A crate type identity, its first type's parameters, and the list of
    /// its types.
    pub(super) const IDENTITY: usize = 11;
    /// A crate type identity and the facade that declares it.
    pub(super) const THROUGH: usize = 9;
}

/// A runner's plan, read where it lies: its header and every table are
/// inside its bytes and of the plan's schema.
#[derive(Clone, Copy)]
pub struct Image<'bytes> {
    bytes: &'bytes [u8],
}

impl<'bytes> Image<'bytes> {
    /// The image `bytes` hold, when they are one of this schema whose
    /// tables lie inside them.
    pub fn read(bytes: &'bytes [u8]) -> Result<Self, super::super::Error> {
        let image = Self { bytes };
        if bytes.get(..MAGIC.len()) != Some(MAGIC.as_slice())
            || bytes.len() < MAGIC.len() + 4 * HEADER_WORDS
            || image.header(0) != super::SCHEMA
        {
            return Err(super::super::Error::Canonical);
        }
        let tables = [
            (image.header(2), image.header(3), stride::BLOB),
            (image.header(4), image.header(5), stride::BLOB),
            (image.header(6), image.header(7), FIELDS * 2),
            (image.header(8), image.header(9), 1),
        ];
        let facades = image.header(1) as usize;
        if tables
            .iter()
            .any(|&(offset, count, stride)| !image.holds(offset, count, stride))
            || image.header(7) as usize != facades
            || image.header(9) as usize != facades
        {
            return Err(super::super::Error::Canonical);
        }
        for index in 0..facades {
            let facade = image.facade(index);
            if !facade.holds_tables() {
                return Err(super::super::Error::Canonical);
            }
        }
        Ok(image)
    }

    /// The `index`th word after the magic.
    fn header(&self, index: usize) -> u32 {
        self.word(MAGIC.len() + 4 * index)
    }

    /// The word at byte `at`, which [`Image::read`] or the table it is in
    /// checked lies inside the image.
    fn word(&self, at: usize) -> u32 {
        let bytes = self
            .bytes
            .get(at..at + 4)
            .expect("a runner image's tables lie inside it, as `Image::read` checked");
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Whether `count` records of `stride` words from `offset` lie inside
    /// the image.
    fn holds(&self, offset: u32, count: u32, stride: usize) -> bool {
        (count as usize)
            .checked_mul(4 * stride)
            .and_then(|length| length.checked_add(offset as usize))
            .is_some_and(|end| end <= self.bytes.len())
    }

    /// The bytes at `offset`, `length` long. Every blob of an image is
    /// inside it: its generator writes it whole.
    fn blob(&self, offset: u32, length: u32) -> &'bytes [u8] {
        self.bytes
            .get(offset as usize..offset as usize + length as usize)
            .expect("a runner image's generator writes each blob inside it")
    }

    /// The text at `offset`, `length` long.
    fn text(&self, offset: u32, length: u32) -> &'bytes str {
        std::str::from_utf8(self.blob(offset, length))
            .expect("a runner image's generator writes its paths as text")
    }
}

/// Write a runner's plan as its image: `plan` is the runner's own, with
/// each re-exported module held once ([`super::Plan::runner_bytes`]).
pub(super) fn write(plan: &super::Plan) -> Result<Vec<u8>, super::super::Error> {
    write::image(plan)
}
