//! What a runner image holds, read one record at a time.

use super::{stride, Field, Image, FIELDS, HEADER_WORDS, MAGIC};
use crate::surface::facade::{ImplHeader, Item, Macro, TraitHeader, TypeAlias, TypeDefinition};

impl<'bytes> Image<'bytes> {
    /// How many facades the image holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.header(1) as usize
    }

    /// Whether the image holds no facade.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The facade at `index`, in root order.
    #[must_use]
    pub fn facade(&self, index: usize) -> Facade<'bytes> {
        let at = self.header(6) as usize + 4 * index * FIELDS * 2;
        Facade { image: *self, at }
    }

    /// The facades' indices in the order a runner builds them: a facade
    /// whose items name another's types after that one.
    pub fn order(&self) -> impl ExactSizeIterator<Item = usize> + 'bytes {
        let image = *self;
        let start = self.header(8) as usize;
        (0..self.len()).map(move |index| image.word(start + 4 * index) as usize)
    }

    /// How many type bodies the image holds, for every facade.
    #[must_use]
    pub fn type_bodies(&self) -> usize {
        self.header(3) as usize
    }

    /// The type at `path` with the body at `body`.
    pub fn type_definition(
        &self,
        path: &str,
        body: u32,
    ) -> Result<TypeDefinition, crate::surface::Error> {
        let (offset, length) = self.record(self.header(2), stride::BLOB, body as usize);
        let body = serde_json::from_slice(self.blob(offset, length))
            .map_err(|_| crate::surface::Error::Encoding)?;
        Ok(TypeDefinition {
            path: path.into(),
            body: std::sync::Arc::new(body),
        })
    }

    /// The item at `path` with the body at `body`.
    pub fn item(&self, path: &str, body: u32) -> Result<Item, crate::surface::Error> {
        let (offset, length) = self.record(self.header(4), stride::BLOB, body as usize);
        let body = serde_json::from_slice(self.blob(offset, length))
            .map_err(|_| crate::surface::Error::Encoding)?;
        Ok(Item {
            path: path.into(),
            body: std::sync::Arc::new(body),
        })
    }

    /// The first two words of record `index` of the table at `offset`.
    fn record(&self, offset: u32, stride: usize, index: usize) -> (u32, u32) {
        let at = offset as usize + 4 * stride * index;
        (self.word(at), self.word(at + 4))
    }

    /// The image's bytes up to its first table, which name it.
    #[must_use]
    pub fn header_bytes(&self) -> &'bytes [u8] {
        &self.bytes[..MAGIC.len() + 4 * HEADER_WORDS]
    }
}

/// The modules a facade reaches at another path, each with the path it is
/// held at.
pub type Reexports = Vec<(Box<str>, Box<str>)>;

/// One facade of an image: its root, its digest, and each table its
/// registry searches, sorted as the registry sorts paths (segment by
/// segment). Every path is relative to the root.
#[derive(Clone, Copy)]
pub struct Facade<'bytes> {
    image: Image<'bytes>,
    at: usize,
}

/// What a facade re-exports at a path that is no module of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A builtin type, as `markdown::Uri` is `String`.
    Builtin,
    /// A std item, as `time::Duration` is `std::time::Duration`.
    Std,
    /// One of the facade's enum variants, as `Center` is
    /// `alignment::Alignment::Center`.
    Variant,
}

/// One re-export of a facade, at `path`, of `target`.
#[derive(Clone, Copy)]
pub struct Alias<'bytes> {
    /// The re-exported path, relative to the root.
    pub path: &'bytes str,
    /// The builtin's name, the std path, or the variant's path relative to
    /// the root.
    pub target: &'bytes str,
    /// What `target` is.
    pub kind: Kind,
}

/// The items one path names: the indices of their bodies, in the order
/// the facade declares them, and of its macros there.
#[derive(Clone, Copy)]
pub struct Group<'bytes> {
    facade: Facade<'bytes>,
    index: usize,
}

/// The types of one crate type identity a facade declares.
#[derive(Clone, Copy)]
pub struct Identities<'bytes> {
    facade: Facade<'bytes>,
    index: usize,
}

impl<'bytes> Facade<'bytes> {
    fn field(&self, field: Field) -> (u32, u32) {
        let at = self.at + 8 * field as usize;
        (self.image.word(at), self.image.word(at + 4))
    }

    /// Word `word` of record `index` of `field`'s table of `stride` words.
    fn word(&self, field: Field, stride: usize, index: usize, word: usize) -> u32 {
        let (offset, _) = self.field(field);
        self.image
            .word(offset as usize + 4 * (stride * index + word))
    }

    /// The text of the blob at words `word` and `word + 1` of a record.
    fn text(&self, field: Field, stride: usize, index: usize, word: usize) -> &'bytes str {
        self.image.text(
            self.word(field, stride, index, word),
            self.word(field, stride, index, word + 1),
        )
    }

    fn bytes(&self, field: Field, stride: usize, index: usize, word: usize) -> &'bytes [u8] {
        self.image.blob(
            self.word(field, stride, index, word),
            self.word(field, stride, index, word + 1),
        )
    }

    /// The `length` bytes record `index` of `field`'s table holds from
    /// its first word on.
    fn inline(&self, field: Field, stride: usize, index: usize, length: usize) -> &'bytes [u8] {
        let (offset, _) = self.field(field);
        let at = offset as usize + 4 * stride * index;
        self.image.blob(
            u32::try_from(at).expect("a runner image fits u32 offsets"),
            u32::try_from(length).expect("a key fits u32"),
        )
    }

    fn count(&self, field: Field) -> usize {
        self.field(field).1 as usize
    }

    fn json<T: serde::de::DeserializeOwned>(
        &self,
        field: Field,
    ) -> Result<T, crate::surface::Error> {
        let (offset, length) = self.field(field);
        serde_json::from_slice(self.image.blob(offset, length))
            .map_err(|_| crate::surface::Error::Encoding)
    }

    /// Whether every table of the facade lies inside its image.
    pub(super) fn holds_tables(&self) -> bool {
        let tables = [
            (Field::Modules, stride::BLOB),
            (Field::Types, stride::TYPE),
            (Field::Groups, stride::GROUP),
            (Field::Aliases, stride::ALIAS),
            (Field::TraitHeaders, stride::HEADER),
            (Field::Impls, stride::BLOB),
            (Field::ImplsByTrait, stride::KEYED),
            (Field::Derives, stride::BLOB),
            (Field::Identities, stride::IDENTITY),
            (Field::Selectors, stride::KEYED),
            (Field::Structural, stride::KEYED),
            (Field::Names, stride::NAME),
            (Field::Through, stride::THROUGH),
        ];
        tables.iter().all(|&(field, stride)| {
            let (offset, count) = self.field(field);
            self.image.holds(offset, count, stride)
        }) && self.image.bytes.len() >= self.field(Field::Digest).0 as usize + 32
    }

    /// The facade's root.
    #[must_use]
    pub fn root(&self) -> &'bytes str {
        let (offset, length) = self.field(Field::Root);
        self.image.text(offset, length)
    }

    /// The digest of everything the facade holds, which its proof carries:
    /// two facades of one root are one contract when it is the same.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let (offset, _) = self.field(Field::Digest);
        let mut digest = [0; 32];
        digest.copy_from_slice(self.image.blob(offset, 32));
        digest
    }

    /// How many modules the facade holds.
    #[must_use]
    pub fn modules(&self) -> usize {
        self.count(Field::Modules)
    }

    /// The module at `index`.
    #[must_use]
    pub fn module(&self, index: usize) -> &'bytes str {
        self.text(Field::Modules, stride::BLOB, index, 0)
    }

    /// How many types the facade holds, at each of their paths.
    #[must_use]
    pub fn types(&self) -> usize {
        self.count(Field::Types)
    }

    /// The path of the type at `index`.
    #[must_use]
    pub fn type_path(&self, index: usize) -> &'bytes str {
        self.text(Field::Types, stride::TYPE, index, 0)
    }

    /// The index of the body of the type at `index`, among the image's
    /// type bodies, which every facade's types share.
    #[must_use]
    pub fn type_body(&self, index: usize) -> usize {
        self.word(Field::Types, stride::TYPE, index, 2) as usize
    }

    /// The type at `index`, read from its body.
    pub fn type_definition(&self, index: usize) -> Result<TypeDefinition, crate::surface::Error> {
        self.image.type_definition(
            self.type_path(index),
            self.word(Field::Types, stride::TYPE, index, 2),
        )
    }

    /// How many paths name items or macros.
    #[must_use]
    pub fn groups(&self) -> usize {
        self.count(Field::Groups)
    }

    /// The items and macros at the path of index `index`.
    #[must_use]
    pub fn group(&self, index: usize) -> Group<'bytes> {
        Group {
            facade: *self,
            index,
        }
    }

    /// How many re-exports the facade has.
    #[must_use]
    pub fn aliases(&self) -> usize {
        self.count(Field::Aliases)
    }

    /// The re-export at `index`.
    #[must_use]
    pub fn alias(&self, index: usize) -> Alias<'bytes> {
        Alias {
            path: self.text(Field::Aliases, stride::ALIAS, index, 0),
            target: self.text(Field::Aliases, stride::ALIAS, index, 2),
            kind: match self.word(Field::Aliases, stride::ALIAS, index, 4) {
                0 => Kind::Builtin,
                1 => Kind::Std,
                _ => Kind::Variant,
            },
        }
    }

    /// How many trait headers the facade has.
    #[must_use]
    pub fn trait_headers(&self) -> usize {
        self.count(Field::TraitHeaders)
    }

    /// The path of the trait header at `index`.
    #[must_use]
    pub fn trait_header_path(&self, index: usize) -> &'bytes str {
        self.text(Field::TraitHeaders, stride::HEADER, index, 0)
    }

    /// The trait header at `index`.
    pub fn trait_header(&self, index: usize) -> Result<TraitHeader, crate::surface::Error> {
        serde_json::from_slice(self.bytes(Field::TraitHeaders, stride::HEADER, index, 2))
            .map_err(|_| crate::surface::Error::Encoding)
    }

    /// How many impl headers the facade has.
    #[must_use]
    pub fn impls(&self) -> usize {
        self.count(Field::Impls)
    }

    /// The impl header at `index`, in the facade's order.
    pub fn impl_header(&self, index: usize) -> Result<ImplHeader, crate::surface::Error> {
        serde_json::from_slice(self.bytes(Field::Impls, stride::BLOB, index, 0))
            .map_err(|_| crate::surface::Error::Encoding)
    }

    /// The indices of the impls of the trait `identity`, in the facade's
    /// order, among them any of a trait whose identity starts as its does.
    pub fn impls_of(&self, identity: &[u8; 32]) -> impl Iterator<Item = usize> + 'bytes {
        self.keyed(Field::ImplsByTrait, identity)
    }

    /// How many derive macros the facade has.
    #[must_use]
    pub fn derives(&self) -> usize {
        self.count(Field::Derives)
    }

    /// The derive macro at `index`.
    #[must_use]
    pub fn derive(&self, index: usize) -> &'bytes str {
        self.text(Field::Derives, stride::BLOB, index, 0)
    }

    /// How many crate type identities the facade declares.
    #[must_use]
    pub fn identities(&self) -> usize {
        self.count(Field::Identities)
    }

    /// The types of the identity at `index`, in identity order.
    #[must_use]
    pub fn identity_at(&self, index: usize) -> Identities<'bytes> {
        Identities {
            facade: *self,
            index,
        }
    }

    /// The types of `identity`, when the facade declares it.
    #[must_use]
    pub fn identity(&self, identity: &[u8; 32]) -> Option<Identities<'bytes>> {
        let count = self.count(Field::Identities);
        let key = |index| self.inline(Field::Identities, stride::IDENTITY, index, 32);
        let index = partition(count, |index| key(index) < identity.as_slice());
        (index < count && key(index) == identity.as_slice()).then(|| self.identity_at(index))
    }

    /// The groups an item of the selector `fingerprint` is in, among them
    /// any of a selector that starts as it does.
    pub fn selected(&self, fingerprint: &[u8; 32]) -> impl Iterator<Item = usize> + 'bytes {
        self.keyed(Field::Selectors, fingerprint)
    }

    /// The types whose constructor, field or variant has the selector
    /// `fingerprint`, in path order, among them any of a selector that
    /// starts as it does.
    pub fn structural(&self, fingerprint: &[u8; 32]) -> impl Iterator<Item = usize> + 'bytes {
        self.keyed(Field::Structural, fingerprint)
    }

    /// The groups whose path ends in `name` and which hold a function.
    pub fn named(&self, name: &str) -> impl Iterator<Item = usize> + 'bytes {
        let facade = *self;
        let count = self.count(Field::Names);
        let start = partition(count, |index| {
            self.text(Field::Names, stride::NAME, index, 0) < name
        });
        let name = name.to_owned();
        (start..count)
            .take_while(move |&index| facade.text(Field::Names, stride::NAME, index, 0) == name)
            .map(move |index| facade.word(Field::Names, stride::NAME, index, 2) as usize)
    }

    /// Each crate type identity the facade's items name that another
    /// facade declares, with that facade's index, in identity order.
    pub fn through(&self) -> impl ExactSizeIterator<Item = ([u8; 32], usize)> + 'bytes {
        let facade = *self;
        (0..self.count(Field::Through)).map(move |index| {
            let mut identity = [0; 32];
            identity.copy_from_slice(facade.inline(Field::Through, stride::THROUGH, index, 32));
            (
                identity,
                facade.word(Field::Through, stride::THROUGH, index, 8) as usize,
            )
        })
    }

    /// The facade's `macro_rules!` macros, in path order.
    pub fn macros(&self) -> Result<Vec<Macro>, crate::surface::Error> {
        self.json(Field::Macros)
    }

    /// The modules the facade reaches at another path, each with the path
    /// it is held at, in path order.
    pub fn reexports(&self) -> Result<Reexports, crate::surface::Error> {
        self.json(Field::Reexports)
    }

    /// The crate's type aliases, other than those of a builtin, in path
    /// order.
    pub fn type_aliases(&self) -> Result<Vec<TypeAlias>, crate::surface::Error> {
        self.json(Field::TypeAliases)
    }

    /// The values of `field`'s keyed table whose key starts as `key` does,
    /// in table order: those of `key`, and of any key that shares its first
    /// four bytes, which each use tells apart.
    fn keyed(&self, field: Field, key: &[u8; 32]) -> impl Iterator<Item = usize> + 'bytes {
        let facade = *self;
        let count = self.count(field);
        let prefix = <[u8; 4]>::try_from(&key[..4]).expect("a key has four bytes");
        let start = partition(count, |index| {
            self.inline(field, stride::KEYED, index, 4) < prefix.as_slice()
        });
        (start..count)
            .take_while(move |&index| facade.inline(field, stride::KEYED, index, 4) == prefix)
            .map(move |index| facade.word(field, stride::KEYED, index, 1) as usize)
    }
}

impl<'bytes> Group<'bytes> {
    /// The path the group's items are at.
    #[must_use]
    pub fn path(&self) -> &'bytes str {
        self.facade
            .text(Field::Groups, stride::GROUP, self.index, 0)
    }

    fn list(&self, word: usize) -> impl ExactSizeIterator<Item = u32> + 'bytes {
        let image = self.facade.image;
        let offset = self
            .facade
            .word(Field::Groups, stride::GROUP, self.index, word) as usize;
        let count = self
            .facade
            .word(Field::Groups, stride::GROUP, self.index, word + 1) as usize;
        (0..count).map(move |index| image.word(offset + 4 * index))
    }

    /// The items at the path, in the order the facade declares them.
    pub fn items(&self) -> Result<Vec<Item>, crate::surface::Error> {
        let path = self.path();
        self.list(2)
            .map(|body| self.facade.image.item(path, body))
            .collect()
    }

    /// The indices, among the facade's macros, of those at the path.
    pub fn macros(&self) -> impl ExactSizeIterator<Item = usize> + 'bytes {
        self.list(4).map(|index| index as usize)
    }
}

impl<'bytes> Identities<'bytes> {
    /// The identity.
    #[must_use]
    pub fn identity(&self) -> [u8; 32] {
        let mut identity = [0; 32];
        identity.copy_from_slice(self.facade.inline(
            Field::Identities,
            stride::IDENTITY,
            self.index,
            32,
        ));
        identity
    }

    /// How many type parameters its first type declares.
    #[must_use]
    pub fn parameters(&self) -> usize {
        self.facade
            .word(Field::Identities, stride::IDENTITY, self.index, 8) as usize
    }

    /// The indices of its types: those at paths through no crate's hidden
    /// segment first, each part in path order.
    pub fn types(&self) -> impl ExactSizeIterator<Item = usize> + 'bytes {
        let image = self.facade.image;
        let offset = self
            .facade
            .word(Field::Identities, stride::IDENTITY, self.index, 9) as usize;
        let count = self
            .facade
            .word(Field::Identities, stride::IDENTITY, self.index, 10) as usize;
        (0..count).map(move |index| image.word(offset + 4 * index) as usize)
    }
}

/// The first of `count` indices at which `before` is false; `before` is
/// true up to some index and false after it.
fn partition(count: usize, before: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, count);
    while low < high {
        let middle = low + (high - low) / 2;
        if before(middle) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    low
}
