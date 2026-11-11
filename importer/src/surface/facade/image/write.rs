//! Writing a runner's plan as its image: each table its registry searches,
//! sorted as the registry sorts paths, and what a runner would otherwise
//! work out at each start, worked out here once.
//!
//! A facade's modules, its kept re-exports and its groups of items are
//! what a runner's facade builder makes of the facade's entries, added in
//! the builder's order (`package::import::runner::build` in rawr): the
//! modules above each type and item, the builtin and std re-exports, the
//! variants, then the modules the program names, the re-exported modules,
//! and the modules above each macro and derive.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use sha2::{Digest as _, Sha256};

use super::{stride, Field, FIELDS, HEADER_WORDS, MAGIC};
use crate::api::ledger::CRATE_SEGMENT;
use crate::surface::facade::{Facade, ItemKind, Plan, Resolution, Shape};

const DIGEST_DOMAIN: &[u8] = b"rawr.import.surface.facade.image/v1";

/// The bytes of an image as it is written, with the bodies every facade
/// shares.
struct Writer {
    bytes: Vec<u8>,
    /// Each blob written, once: a path, a header or a key two facades hold
    /// alike is one blob.
    blobs: HashMap<Vec<u8>, (u32, u32)>,
    type_bodies: BodyTable,
    item_bodies: BodyTable,
}

/// Each distinct body once, by its written JSON, with its index.
#[derive(Default)]
struct BodyTable {
    index: HashMap<Vec<u8>, u32>,
    blobs: Vec<(u32, u32)>,
}

pub(super) fn image(plan: &Plan) -> Result<Vec<u8>, crate::surface::Error> {
    let mut writer = Writer {
        bytes: Vec::new(),
        blobs: HashMap::new(),
        type_bodies: BodyTable::default(),
        item_bodies: BodyTable::default(),
    };
    writer.bytes.extend_from_slice(MAGIC);
    writer.bytes.resize(MAGIC.len() + 4 * HEADER_WORDS, 0);
    let facades = plan.facades.iter().collect::<Vec<_>>();
    let resolution = Resolution::new(
        &facades
            .iter()
            .map(|facade| facade.types().map(|ty| *ty.identity()).collect())
            .collect::<Vec<_>>(),
        &facades
            .iter()
            .map(|facade| facade.named())
            .collect::<Vec<_>>(),
    );
    let mut records = Vec::with_capacity(facades.len());
    for (index, facade) in facades.iter().enumerate() {
        records.push(writer.facade(facade, resolution.through(index))?);
    }
    let type_bodies = writer.table(
        &writer
            .type_bodies
            .blobs
            .iter()
            .flat_map(|&(offset, length)| [offset, length])
            .collect::<Vec<_>>(),
    );
    let item_bodies = writer.table(
        &writer
            .item_bodies
            .blobs
            .iter()
            .flat_map(|&(offset, length)| [offset, length])
            .collect::<Vec<_>>(),
    );
    let facade_table = writer.table(&records.concat());
    let order = writer.table(
        &resolution
            .order()
            .iter()
            .map(|&index| word(index))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let header = [
        super::super::SCHEMA,
        word(facades.len())?,
        type_bodies,
        word(writer.type_bodies.blobs.len())?,
        item_bodies,
        word(writer.item_bodies.blobs.len())?,
        facade_table,
        word(facades.len())?,
        order,
        word(facades.len())?,
        0,
    ];
    for (index, value) in header.iter().enumerate() {
        let at = MAGIC.len() + 4 * index;
        writer.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(writer.bytes)
}

fn word(value: usize) -> Result<u32, crate::surface::Error> {
    u32::try_from(value).map_err(|_| crate::surface::Error::Encoding)
}

/// A path's segments, which the registry orders paths by.
fn segments(path: &str) -> Vec<&str> {
    path.split("::").collect()
}

/// Whether a path goes through a crate's hidden segment.
fn hidden(path: &str) -> bool {
    path.contains(CRATE_SEGMENT)
}

/// Each strict prefix of `path`, shortest first.
fn prefixes(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices("::").map(|(end, _)| &path[..end])
}

impl Writer {
    /// Append `bytes`, unless the image holds them already, and give
    /// their offset and length.
    fn blob(&mut self, bytes: &[u8]) -> Result<(u32, u32), crate::surface::Error> {
        if let Some(&blob) = self.blobs.get(bytes) {
            return Ok(blob);
        }
        let offset = word(self.bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        let blob = (offset, word(bytes.len())?);
        self.blobs.insert(bytes.to_vec(), blob);
        Ok(blob)
    }

    /// Append `words` and give their offset.
    fn table(&mut self, words: &[u32]) -> u32 {
        let offset = u32::try_from(self.bytes.len()).expect("a runner image fits u32 offsets");
        for value in words {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        offset
    }

    /// Append the JSON of `value` and give its blob.
    fn json(&mut self, value: &impl serde::Serialize) -> Result<(u32, u32), crate::surface::Error> {
        let written = serde_json::to_vec(value).map_err(|_| crate::surface::Error::Encoding)?;
        self.blob(&written)
    }

    fn type_body(&mut self, body: &impl serde::Serialize) -> Result<u32, crate::surface::Error> {
        let written = serde_json::to_vec(body).map_err(|_| crate::surface::Error::Encoding)?;
        if let Some(&index) = self.type_bodies.index.get(&written) {
            return Ok(index);
        }
        let blob = self.blob(&written)?;
        let index = word(self.type_bodies.blobs.len())?;
        self.type_bodies.blobs.push(blob);
        self.type_bodies.index.insert(written, index);
        Ok(index)
    }

    fn item_body(&mut self, body: &impl serde::Serialize) -> Result<u32, crate::surface::Error> {
        let written = serde_json::to_vec(body).map_err(|_| crate::surface::Error::Encoding)?;
        if let Some(&index) = self.item_bodies.index.get(&written) {
            return Ok(index);
        }
        let blob = self.blob(&written)?;
        let index = word(self.item_bodies.blobs.len())?;
        self.item_bodies.blobs.push(blob);
        self.item_bodies.index.insert(written, index);
        Ok(index)
    }

    /// Write `facade`'s tables and give its record's words.
    fn facade(
        &mut self,
        facade: &Facade,
        through: &[([u8; 32], usize)],
    ) -> Result<Vec<u32>, crate::surface::Error> {
        let built = Built::of(facade);
        let mut fields = [(0_u32, 0_u32); FIELDS];
        fields[Field::Root as usize] = self.blob(facade.root.as_bytes())?;
        fields[Field::Digest as usize] = self.blob(&digest(facade)?)?;

        let mut words = Vec::new();
        for module in &built.modules {
            let (offset, length) = self.blob(module.as_bytes())?;
            words.extend([offset, length]);
        }
        fields[Field::Modules as usize] = (self.table(&words), word(built.modules.len())?);

        // Types, in path order.
        let mut types = facade.types.iter().collect::<Vec<_>>();
        types.sort_by(|left, right| segments(&left.path).cmp(&segments(&right.path)));
        let mut words = Vec::new();
        for ty in &types {
            let (offset, length) = self.blob(ty.path.as_bytes())?;
            let body = self.type_body(&*ty.body)?;
            words.extend([offset, length, body]);
        }
        fields[Field::Types as usize] = (self.table(&words), word(types.len())?);

        // Groups: each path's items, in the facade's order, its macros, the
        // selectors of its items and whether one is a function.
        let mut groups = BTreeMap::<Vec<&str>, Group<'_>>::new();
        for item in facade.items.iter() {
            let body = self.item_body(&*item.body)?;
            let group = groups
                .entry(segments(&item.path))
                .or_insert_with(|| Group::at(&item.path));
            group.items.push(body);
            group.selectors.push(item.body.selector.fingerprint());
            group.function |= matches!(item.body.kind, ItemKind::Function);
        }
        for (index, rules) in facade.macros.iter().enumerate() {
            groups
                .entry(segments(&rules.path))
                .or_insert_with(|| Group::at(&rules.path))
                .macros
                .push(word(index)?);
        }
        let mut words = Vec::new();
        let mut selectors = Vec::new();
        let mut names = Vec::new();
        for (index, group) in groups.values().enumerate() {
            let (offset, length) = self.blob(group.path.as_bytes())?;
            let item_list = self.table(&group.items);
            let macro_list = self.table(&group.macros);
            words.extend([
                offset,
                length,
                item_list,
                word(group.items.len())?,
                macro_list,
                word(group.macros.len())?,
            ]);
            let index = word(index)?;
            selectors.extend(group.selectors.iter().map(|selector| (*selector, index)));
            if group.function {
                let name = group.path.rsplit("::").next().unwrap_or(group.path);
                names.push((name, index));
            }
        }
        fields[Field::Groups as usize] = (self.table(&words), word(groups.len())?);
        selectors.sort();
        selectors.dedup();
        fields[Field::Selectors as usize] = self.keyed(&selectors)?;
        names.sort();
        let mut words = Vec::new();
        for (name, group) in &names {
            let (offset, length) = self.blob(name.as_bytes())?;
            words.extend([offset, length, *group]);
        }
        fields[Field::Names as usize] = (self.table(&words), word(names.len())?);

        // Re-exports at paths that are no modules.
        let mut words = Vec::new();
        for (path, (target, kind)) in &built.aliases {
            let (path_offset, path_length) = self.blob(path.join("::").as_bytes())?;
            let (target_offset, target_length) = self.blob(target.as_bytes())?;
            words.extend([
                path_offset,
                path_length,
                target_offset,
                target_length,
                *kind,
            ]);
        }
        fields[Field::Aliases as usize] = (self.table(&words), word(built.aliases.len())?);

        // Trait headers, in path order.
        let mut headers = facade.trait_headers.iter().collect::<Vec<_>>();
        headers.sort_by(|left, right| segments(&left.path).cmp(&segments(&right.path)));
        let mut words = Vec::new();
        for header in &headers {
            let (path_offset, path_length) = self.blob(header.path.as_bytes())?;
            let (offset, length) = self.json(header)?;
            words.extend([path_offset, path_length, offset, length]);
        }
        fields[Field::TraitHeaders as usize] = (self.table(&words), word(headers.len())?);

        // Impl headers, in the facade's order, and by trait.
        let mut words = Vec::new();
        let mut by_trait = Vec::new();
        for (index, header) in facade.impls.iter().enumerate() {
            let (offset, length) = self.json(header)?;
            words.extend([offset, length]);
            by_trait.push((header.trait_identity, word(index)?));
        }
        fields[Field::Impls as usize] = (self.table(&words), word(facade.impls.len())?);
        by_trait.sort();
        fields[Field::ImplsByTrait as usize] = self.keyed(&by_trait)?;

        // Derives, sorted and distinct.
        let derives = facade
            .derives
            .iter()
            .map(|path| (segments(path), &**path))
            .collect::<BTreeMap<_, _>>();
        let mut words = Vec::new();
        for path in derives.values() {
            let (offset, length) = self.blob(path.as_bytes())?;
            words.extend([offset, length]);
        }
        fields[Field::Derives as usize] = (self.table(&words), word(derives.len())?);

        // Each identity's types: public paths first, each part in path order.
        let mut identities = BTreeMap::<[u8; 32], (usize, Vec<(bool, u32)>)>::new();
        let mut structural = Vec::new();
        for (index, ty) in types.iter().enumerate() {
            let index = word(index)?;
            identities
                .entry(ty.body.identity)
                .or_insert_with(|| (ty.body.parameters.len(), Vec::new()))
                .1
                .push((hidden(&ty.path), index));
            match &ty.body.shape {
                Shape::Struct {
                    constructor,
                    fields,
                    ..
                } => {
                    structural.extend(
                        constructor
                            .iter()
                            .map(|selector| (selector.fingerprint(), index)),
                    );
                    structural.extend(
                        fields
                            .iter()
                            .map(|field| (field.selector.fingerprint(), index)),
                    );
                }
                Shape::Enum { variants, .. } => structural.extend(
                    variants
                        .iter()
                        .map(|variant| (variant.selector.fingerprint(), index)),
                ),
            }
        }
        let mut words = Vec::new();
        for (identity, (parameters, mut paths)) in identities {
            paths.sort();
            let list = self.table(&paths.iter().map(|&(_, index)| index).collect::<Vec<_>>());
            words.extend(key_words(&identity));
            words.extend([word(parameters)?, list, word(paths.len())?]);
        }
        let count = word(words.len() / stride::IDENTITY)?;
        fields[Field::Identities as usize] = (self.table(&words), count);
        structural.sort();
        structural.dedup();
        fields[Field::Structural as usize] = self.keyed(&structural)?;
        let mut words = Vec::new();
        for &(identity, provider) in through {
            words.extend(key_words(&identity));
            words.push(word(provider)?);
        }
        fields[Field::Through as usize] = (self.table(&words), word(through.len())?);

        fields[Field::Macros as usize] = self.json(&facade.macros)?;
        fields[Field::Reexports as usize] = self.json(&facade.reexports)?;
        fields[Field::TypeAliases as usize] = self.json(&facade.type_aliases)?;
        Ok(fields
            .iter()
            .flat_map(|&(offset, count)| [offset, count])
            .collect())
    }

    /// A keyed table of `entries`, sorted, each its key's first four bytes
    /// and its value.
    fn keyed(&mut self, entries: &[([u8; 32], u32)]) -> Result<(u32, u32), crate::surface::Error> {
        let mut words = Vec::new();
        for (key, value) in entries {
            words.extend(key_words(&key[..4]));
            words.push(*value);
        }
        debug_assert_eq!(words.len(), entries.len() * stride::KEYED);
        Ok((self.table(&words), word(entries.len())?))
    }
}

/// `key` as the words a table holds it in, so its bytes lie in the image
/// as they are.
fn key_words(key: &[u8]) -> impl Iterator<Item = u32> + '_ {
    key.chunks(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

/// The digest of everything `facade` holds, as its plan writes it.
fn digest(facade: &Facade) -> Result<[u8; 32], crate::surface::Error> {
    let written = serde_json::to_vec(facade).map_err(|_| crate::surface::Error::Encoding)?;
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    digest.update(super::super::SCHEMA.to_le_bytes());
    digest.update(written);
    Ok(digest.finalize().into())
}

/// The items and macros one path names, as an image's group is written.
struct Group<'facade> {
    path: &'facade str,
    items: Vec<u32>,
    macros: Vec<u32>,
    selectors: Vec<[u8; 32]>,
    function: bool,
}

impl<'facade> Group<'facade> {
    fn at(path: &'facade str) -> Self {
        Self {
            path,
            items: Vec::new(),
            macros: Vec::new(),
            selectors: Vec::new(),
            function: false,
        }
    }
}

/// What a runner's facade builder makes of a facade's paths: its modules,
/// and the re-exports it keeps, each with its target and kind (0 a
/// builtin, 1 a std item, 2 a variant).
struct Built<'facade> {
    modules: Vec<String>,
    aliases: BTreeMap<Vec<&'facade str>, (&'facade str, u32)>,
}

impl<'facade> Built<'facade> {
    fn of(facade: &'facade Facade) -> Self {
        let types = facade
            .types
            .iter()
            .map(|ty| &*ty.path)
            .collect::<BTreeSet<_>>();
        // An alias's associated functions sit under the alias, which is no
        // module.
        let owners = types
            .iter()
            .copied()
            .chain(facade.type_aliases.iter().map(|alias| &*alias.path))
            .collect::<BTreeSet<_>>();
        let mut modules = BTreeSet::<&str>::new();
        for path in facade
            .types
            .iter()
            .map(|ty| &*ty.path)
            .chain(facade.items.iter().map(|item| &*item.path))
        {
            for candidate in prefixes(path) {
                if owners.contains(candidate) {
                    break;
                }
                modules.insert(candidate);
            }
            if owners.contains(path) {
                modules.remove(path);
            }
        }
        let mut aliases = BTreeMap::<&str, (&str, u32)>::new();
        for (path, builtin) in facade.builtins.iter() {
            aliases.insert(path, (builtin, 0));
        }
        // A std re-export at a path the facade holds something at gives way.
        for (path, std) in facade.std_reexports.iter() {
            let std_path = matches!(segments(std).as_slice(), ["std", _, ..]);
            if std_path
                && !modules.contains(&**path)
                && !types.contains(&**path)
                && !aliases.contains_key(&**path)
            {
                aliases.insert(path, (std, 1));
            }
        }
        for (path, variant) in facade.variants.iter() {
            aliases.insert(path, (variant, 2));
        }
        // A module named in a `use`, each of its prefixes first, up to one
        // that is a type or a re-export.
        let module = |modules: &mut BTreeSet<&'facade str>, path: &'facade str| -> bool {
            if modules.contains(path) {
                return true;
            }
            if types.contains(path) || aliases.contains_key(path) {
                return false;
            }
            modules.insert(path);
            true
        };
        for path in facade.traits.iter() {
            for candidate in prefixes(path).chain(std::iter::once(&**path)) {
                if !module(&mut modules, candidate) {
                    break;
                }
            }
        }
        for (path, _) in facade.reexports.iter() {
            module(&mut modules, path);
        }
        for path in facade
            .macros
            .iter()
            .map(|rules| &*rules.path)
            .chain(facade.derives.iter().map(|path| &**path))
        {
            for candidate in prefixes(path) {
                if !module(&mut modules, candidate) {
                    break;
                }
            }
        }
        let mut modules = modules.into_iter().collect::<Vec<_>>();
        modules.sort_by_key(|path| segments(path));
        Built {
            modules: modules.into_iter().map(str::to_owned).collect(),
            aliases: aliases
                .into_iter()
                .map(|(path, target)| (segments(path), target))
                .collect(),
        }
    }
}
