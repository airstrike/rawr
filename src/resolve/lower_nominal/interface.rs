//! What each source's lowering reads of the program (#1646, #1718): what
//! every source's reads, and the declarations its own text reaches by
//! name.
//!
//! A source's lowering never reads another source's function and method
//! bodies, their inert attributes, or where that source writes its
//! declarations. It reads a declaration of another source only through a
//! name: a path, a method, a type a signature or a field names, or an impl
//! of such a type or trait. An impl is reached by its type's and its
//! trait's names, not its methods': a source calls a method on a value of
//! a type it reaches, so it reaches the impl through the type (#1725).
//!
//! So each declaration, an item with what the program resolved of it, is
//! digested on its own, its places counted in its own tokens
//! (`Places::between`) and its item and method ids left unnumbered
//! (`item::unnumbered`): the lowering's generated names never hold them.
//! A facade's proof is written as its root (`Fingerprint::rooted`): what a
//! source reads of a facade is its entries, below. A source reads each
//! declaration that declares a name its text writes, then each declaring
//! a name one of those writes, and so on.
//! A name that no declaration declares reaches the one a later save adds.
//! An impl of a type parameter (`impl<T> Trait for T`) is every source's.
//!
//! Each `macro_rules!` definition is one more declaration, reached by its
//! name, as a call writes it, and digested by its own tokens' places, so a
//! declaration a save adds above it keeps every other source's key
//! (#1730). What every lookup reads stays every source's: the recursion
//! limit, which bounds each expansion, and which module each source is a
//! child of, which each lookup walks up.
//!
//! A source reads the native surface the same way (#1725). Each entry of
//! the facades of the program's own crate and of each path crate, which a
//! save to the program or to a crate of the project changes, is one more
//! declaration, reached by its names (`host::image::Outline`), so an edit
//! to a path crate lowers again only the sources that name what changed
//! (#1735). A registry crate's facade is every source's, by its proof: it
//! doesn't change while the project runs. Each marked type the runner
//! holds as its mirror (`Program::mirrors`) is one more declaration too,
//! reached by its name, so a new pub struct native code sees reaches only
//! the sources that name it (#1740). So a new runner's start lowers again
//! only the sources that name what changed, as a save in the running one
//! does.

use super::models::Models;
use crate::host::image;
use crate::program::source;
use crate::resolve::{item, type_ref, Program};
use crate::tokenizer::Token;
use crate::types::position::Places;
use crate::{Engine, ImmutableString, Position, Span};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// What each source's lowering reads of the program, the other sources'
/// bodies aside.
pub(super) struct Interface {
    /// What every source's lowering reads.
    every: [u8; 32],
    /// What each source's lowering reads, `every` among it.
    sources: BTreeMap<source::Id, [u8; 32]>,
}

impl Interface {
    /// What `source`'s lowering reads of the program.
    pub(super) fn of(&self, source: source::Id) -> [u8; 32] {
        self.sources.get(&source).copied().unwrap_or(self.every)
    }

    /// What each source's lowering reads of `program`; `None` when it
    /// can't be written. `outlined` keeps the outlines of the sources'
    /// texts, and `digests` the declarations' digests, from one interface
    /// to the next.
    pub(super) fn new(
        engine: &Engine,
        program: &Program,
        models: &Models,
        outlined: &mut Outlined,
        digests: &mut Digests,
        native: &Native,
    ) -> Option<Self> {
        let outlines = outlines(engine, program, outlined);
        let declarations = Declarations::new(program, &outlines, native, digests)?;
        let own = own(program)?;
        // A runner's compiler reads the facades registered in the process
        // as its image holds them; a check reads its own registry's alone,
        // which the facades below write whole, so another engine's
        // registration keeps every source's key (#1850).
        let registered = engine
            .imported_native
            .is_some()
            .then(Engine::registered_facades);
        let every = every(
            program,
            registered,
            models,
            &outlines,
            &declarations,
            own.get(&None),
            native,
        )?;
        let mut sources = BTreeMap::new();
        let mut walk = Walk::new(&declarations);
        for (index, outline) in outlines.sources.iter().enumerate() {
            let source = source::Id::from_index(index);
            let mut written = Written(Sha256::new());
            written.0.update(every);
            declarations.reached(&outline.mentioned, &mut walk, &mut written.0);
            // What the program resolved of the source's own names.
            let aliases = program
                .aliases
                .range(from(source))
                .take_while(|((at, _), _)| *at == source);
            for ((at, name), alias) in aliases {
                outline
                    .places
                    .writing(|| write!(written, "{at:?}|{name:?}|{alias:?}|"))
                    .ok()?;
            }
            let trait_names = program
                .trait_names
                .range(from(source))
                .take_while(|((at, _), _)| *at == source);
            for ((at, name), path) in trait_names {
                write!(written, "{at:?}|{name:?}|{path:?}|").ok()?;
            }
            written.0.update(own.get(&Some(source)).unwrap_or(&[0; 32]));
            sources.insert(source, written.0.finalize().into());
        }
        Some(Self { every, sources })
    }
}

/// Where `source`'s entries of a map by source and name start.
fn from(source: source::Id) -> std::ops::RangeFrom<(source::Id, ImmutableString)> {
    (source, ImmutableString::new())..
}

/// What a program's lowering reads of the native surface beside what a
/// name reaches: the roots of the facades its image holds of the
/// program's own crates and of the project's path crates, whose entries a
/// name reaches (`image::Outline`), and what no name reaches of them.
pub(super) struct Native {
    /// The program's own crates, which it was read for (`Program::own`).
    own: Box<[crate::ImmutableString]>,
    roots: Box<[Box<str>]>,
    outline: Option<image::Outline>,
}

impl Native {
    /// What `program`'s lowering reads of `registry`'s facades.
    pub(super) fn new(program: &Program, registry: &crate::host::Registry) -> Self {
        let held = registry
            .held_facades()
            .filter(|held| {
                held.is_path_crate()
                    || program
                        .own
                        .iter()
                        .any(|own| own.as_str() == held.root().to_string())
            })
            .collect::<Vec<_>>();
        Self {
            own: program.own.clone(),
            roots: held
                .iter()
                .map(|held| held.root().to_string().into())
                .collect(),
            outline: (!held.is_empty()).then(|| image::Outline::new(&held)),
        }
    }

    /// Whether it was read for a program whose own crates are `own`.
    pub(super) fn is_of(&self, own: &[crate::ImmutableString]) -> bool {
        *self.own == *own
    }

    fn entries(&self) -> &[image::outline::Entry] {
        self.outline.as_ref().map_or(&[], image::Outline::entries)
    }
}

/// What every source's lowering reads: the compiler, what the program holds
/// for all its sources, and the impls of a type parameter.
fn every(
    program: &Program,
    registered: Option<usize>,
    models: &Models,
    outlines: &Outlines,
    declarations: &Declarations<'_>,
    own: Option<&[u8; 32]>,
    native: &Native,
) -> Option<[u8; 32]> {
    let mut written = Written(Sha256::new());
    written.0.update(b"rawr.lowering.interface/v10");
    // The compiler: the facades its host registry holds, and its options.
    if let Some(registered) = registered {
        write!(written, "{registered}").ok()?;
    }
    write!(
        written,
        "|{:?}|{:?}|{:?}",
        models.access, models.features, models.std_traits,
    )
    .ok()?;
    // The native surface: each registry crate's facade whole, by its
    // proof, and what no name reaches of the others (#1725, #1735).
    for facade in models.host.facades() {
        framed(&mut written.0, facade.root());
        if !native
            .roots
            .iter()
            .any(|root| root.as_ref() == facade.root())
        {
            written.0.update(facade.fingerprint().content());
        }
    }
    if let Some(outline) = &native.outline {
        written.0.update(outline.everywhere());
    }
    // Which source each index is.
    for index in 0..outlines.sources.len() {
        framed(
            &mut written.0,
            program.source.identity_at(source::Id::from_index(index)),
        );
    }
    write!(
        written,
        "{:?}|{:?}|{:?}|{:?}",
        program.package, program.std_traits, program.entry_crate, program.crates,
    )
    .ok()?;
    // Each source's statements outside its items, its items blank: by its
    // tokens, as a body's length changes how many blanks it is, and a
    // comment is none.
    for (script, outline) in program.scripts.iter().zip(&outlines.scripts) {
        write!(written, "{:?}|{:?}|", script.source, script.namespace).ok()?;
        written.0.update(outline.tokens);
    }
    // What every macro lookup reads (#1730), and the `IndexMut` bodies,
    // which a body's lowering reads with their places: at those places.
    program.macros.write_interface(&mut written).ok()?;
    write!(written, "{:?}", models.index_mut).ok()?;
    // What a call whose body isn't one of the program's resolved.
    written.0.update(own.unwrap_or(&[0; 32]));
    written.0.update(declarations.everywhere);
    Some(written.0.finalize().into())
}

/// The program's declarations, each digested on its own, by the names that
/// reach them.
struct Declarations<'p> {
    /// Each name some declaration declares, by its number.
    names: HashMap<&'p str, usize>,
    /// The declarations that declare each name, by its number.
    declaring: Vec<Vec<usize>>,
    declarations: Vec<Declaration>,
    /// What every source reads of them: the impls of a type parameter, and
    /// what the program holds of no item.
    everywhere: [u8; 32],
}

/// One declaration: an item and what the program resolved of it.
struct Declaration {
    digest: [u8; 32],
    /// The numbers of the declared names its declaration writes, which a
    /// source that reaches it reaches too.
    writes: Vec<usize>,
}

impl<'p> Declarations<'p> {
    /// The program's declarations, whose sources' outlines are `outlines`,
    /// and the entries of its own facades, `native`'s. An item `digests`
    /// digested as it is now isn't digested again.
    fn new(
        program: &'p Program,
        outlines: &'p Outlines,
        native: &'p Native,
        digests: &mut Digests,
    ) -> Option<Self> {
        let items = program.items.iter().collect::<Vec<_>>();
        // What the program holds of each item beside its shell: its type,
        // the methods its impl or trait declares, its instances.
        let mut held = vec![Vec::<Held<'p>>::new(); items.len()];
        let mut unheld = Vec::<Held<'p>>::new();
        let by_id = items
            .iter()
            .enumerate()
            .map(|(index, shell)| (shell.id, index))
            .collect::<HashMap<_, _>>();
        for definition in program.types.iter() {
            match by_id.get(&definition.item()) {
                Some(index) => held[*index].push(Held::Type(definition)),
                None => unheld.push(Held::Type(definition)),
            }
        }
        let within = Within::new(&items);
        let mut by_key = items
            .iter()
            .enumerate()
            .filter_map(|(index, shell)| Some((shell.key.as_ref()?, index)))
            .collect::<HashMap<_, _>>();
        for definition in program.methods.iter().chain(&program.builtin_methods) {
            match within.item(definition.source(), definition.name_span().start()) {
                Some(index) => {
                    by_key.entry(definition.key()).or_insert(index);
                    held[index].push(Held::Method(definition));
                }
                None => unheld.push(Held::Method(definition)),
            }
        }
        for (index, instances) in &program.instances.impls {
            match held.get_mut(*index) {
                Some(held) => held.push(Held::Impl(instances)),
                None => unheld.push(Held::Impl(instances)),
            }
        }
        // A template's instances, which reveal found its sources' calls
        // take: the template's, as a source that names it reads them.
        for (key, instances) in &program.instances.templates {
            match by_key.get(key) {
                Some(index) => held[*index].push(Held::Template(key, instances)),
                None => unheld.push(Held::Template(key, instances)),
            }
        }
        for (key, types) in &program.instances.types {
            match by_key.get(key) {
                Some(index) => held[*index].push(Held::Instance(key, types)),
                None => unheld.push(Held::Instance(key, types)),
            }
        }
        // An instance native code calls a template at, the template's, by
        // the path native code's call names it by (#1725).
        #[cfg(not(target_arch = "wasm32"))]
        {
            let by_path = by_key
                .iter()
                .filter_map(|(key, index)| Some((key.island_path()?.1, *index)))
                .collect::<HashMap<_, _>>();
            for native in &program.instances.natives {
                match by_path.get(native.path.as_str()) {
                    Some(index) => held[*index].push(Held::Native(native)),
                    None => unheld.push(Held::Native(native)),
                }
            }
        }

        let mut names = HashMap::<&str, usize>::new();
        let mut declaring = Vec::<Vec<usize>>::new();
        let mut everywhere = Written(Sha256::new());
        let mut unnamed = Vec::with_capacity(items.len());
        for (index, shell) in items.iter().enumerate() {
            let named = declared_names(&shell.kind);
            for name in &named {
                let number = *names.entry(name).or_insert_with(|| {
                    declaring.push(Vec::new());
                    declaring.len() - 1
                });
                declaring[number].push(index);
            }
            unnamed.push(named.is_empty());
        }
        // Each entry of the program's own facades, after the items. One no
        // name reaches is in what every source reads of them.
        for (offset, entry) in native.entries().iter().enumerate() {
            for name in entry.names() {
                let number = *names.entry(name).or_insert_with(|| {
                    declaring.push(Vec::new());
                    declaring.len() - 1
                });
                declaring[number].push(items.len() + offset);
            }
        }
        // Each marked type the runner holds as its mirror, after them: by
        // its name, as a source that holds a value of it names the type or
        // reaches a declaration that does (#1740).
        let mirrors = items.len() + native.entries().len();
        for (offset, (path, rust)) in program.mirrors.iter().enumerate() {
            let named = path.last().into_iter().chain(rust.last());
            for name in named {
                let number = *names.entry(name.as_str()).or_insert_with(|| {
                    declaring.push(Vec::new());
                    declaring.len() - 1
                });
                declaring[number].push(mirrors + offset);
            }
        }
        // Each `macro_rules!` definition, after them: by its name, as a
        // call writes it (#1730).
        let macros = mirrors + program.mirrors.len();
        for (offset, definition) in program.macros.definitions().enumerate() {
            let number = *names.entry(definition.name()).or_insert_with(|| {
                declaring.push(Vec::new());
                declaring.len() - 1
            });
            declaring[number].push(macros + offset);
        }
        let mut declarations = Vec::with_capacity(macros + program.macros.definitions().count());
        digests.begin();
        for ((index, shell), held) in items.iter().enumerate().zip(&held) {
            let outline = outlines.sources.get(shell.source.index());
            let identity = program.source.identity_at(shell.source);
            let item = Item {
                outline: outline.map(|outline| outline.key),
                identity,
                shell,
                held,
            };
            let digest = digests.digest(item, || {
                let places = outline
                    .map(|outline| outline.places.between(shell.span.start(), shell.span.end()));
                digest(places.as_ref(), |written| {
                    let kind = outlined(&shell.kind);
                    write!(
                        written,
                        "{identity}|{:?}|{:?}|{:?}|{kind:?}|",
                        shell.key, shell.span, shell.facades,
                    )?;
                    held.iter()
                        .try_for_each(|held| write!(written, "{held:?}|"))
                })
            })?;
            let mut writes = outline
                .into_iter()
                .flat_map(|outline| outline.identifiers(shell.span))
                .filter_map(|name| names.get(name).copied())
                .collect::<Vec<_>>();
            writes.sort_unstable();
            writes.dedup();
            if unnamed[index] {
                everywhere.0.update(digest);
            }
            declarations.push(Declaration { digest, writes });
        }
        for entry in native.entries() {
            let mut writes = entry
                .writes()
                .filter_map(|name| names.get(name).copied())
                .collect::<Vec<_>>();
            writes.sort_unstable();
            writes.dedup();
            declarations.push(Declaration {
                digest: *entry.digest(),
                writes,
            });
        }
        for mirror in &program.mirrors {
            declarations.push(Declaration {
                digest: digest(None, |written| write!(written, "{mirror:?}"))?,
                writes: Vec::new(),
            });
        }
        for definition in program.macros.definitions() {
            let span = definition.span();
            let outline = outlines.sources.get(definition.source().index());
            let places = outline.map(|outline| outline.places.between(span.start(), span.end()));
            // Its text alone: `source_span` keeps the lines before it, which
            // a declaration a save adds above it would change.
            let text =
                super::tokens::source_span(program.source.text_at(definition.source()), span);
            let text = text.trim();
            let digest = digest(places.as_ref(), |written| {
                write!(
                    written,
                    "{}|",
                    program.source.identity_at(definition.source())
                )?;
                program.macros.write_definition(definition, written)?;
                write!(written, "{}|{text}", text.len())
            })?;
            let mut writes = outline
                .into_iter()
                .flat_map(|outline| outline.identifiers(span))
                .filter_map(|name| names.get(name).copied())
                .collect::<Vec<_>>();
            writes.sort_unstable();
            writes.dedup();
            declarations.push(Declaration { digest, writes });
        }
        let unheld = digest(None, |written| {
            unheld
                .iter()
                .try_for_each(|held| write!(written, "{held:?}|"))
        })?;
        everywhere.0.update(unheld);
        Some(Self {
            names,
            declaring,
            declarations,
            everywhere: everywhere.0.finalize().into(),
        })
    }

    /// Write into `digest` the declarations a text that writes the names
    /// `mentioned` reaches: how many, and their digests summed lane by
    /// lane, which no order of the walk changes. The walk touches only
    /// what it reaches, and no digest is digested again (#1850).
    fn reached(&self, mentioned: &[Box<str>], walk: &mut Walk, digest: &mut Sha256) {
        let mut sum = [0_u64; 4];
        let mut count = 0_u64;
        for name in mentioned {
            if let Some(&name) = self.names.get(name.as_ref()) {
                walk.see(name);
            }
        }
        while let Some(name) = walk.next.pop() {
            for &index in &self.declaring[name] {
                if std::mem::replace(&mut walk.reached[index], true) {
                    continue;
                }
                walk.found.push(index);
                count += 1;
                let (lanes, _) = self.declarations[index].digest.as_chunks::<8>();
                for (lane, bytes) in sum.iter_mut().zip(lanes) {
                    *lane = lane.wrapping_add(u64::from_le_bytes(*bytes));
                }
                for &written in &self.declarations[index].writes {
                    walk.see(written);
                }
            }
        }
        walk.clear();
        digest.update(count.to_le_bytes());
        for lane in sum {
            digest.update(lane.to_le_bytes());
        }
    }
}

/// What one walk over the declarations marked (`Declarations::reached`),
/// unmarked after it, so the next walk marks only what it reaches.
struct Walk {
    /// Whether the walk saw each declared name, by its number.
    seen: Vec<bool>,
    /// Whether it reached each declaration, by its number.
    reached: Vec<bool>,
    /// The names it saw, and those it has yet to follow.
    names: Vec<usize>,
    next: Vec<usize>,
    /// The declarations it reached.
    found: Vec<usize>,
}

impl Walk {
    fn new(declarations: &Declarations<'_>) -> Self {
        Self {
            seen: vec![false; declarations.declaring.len()],
            reached: vec![false; declarations.declarations.len()],
            names: Vec::new(),
            next: Vec::new(),
            found: Vec::new(),
        }
    }

    /// Follow the name `name` next, unless the walk saw it.
    fn see(&mut self, name: usize) {
        if !std::mem::replace(&mut self.seen[name], true) {
            self.names.push(name);
            self.next.push(name);
        }
    }

    /// Unmark what the walk marked.
    fn clear(&mut self) {
        for name in self.names.drain(..) {
            self.seen[name] = false;
        }
        for index in self.found.drain(..) {
            self.reached[index] = false;
        }
    }
}

/// What the program holds of an item beside its shell.
#[derive(Clone, Copy, Debug)]
enum Held<'p> {
    Type(&'p crate::type_def::Definition),
    Method(&'p crate::method::Definition),
    Impl(&'p [crate::resolve::ImplInstance]),
    Template(&'p crate::item::Key, &'p [crate::resolve::Instance]),
    Instance(
        &'p crate::item::Key,
        &'p [(crate::ImmutableString, type_ref::Type)],
    ),
    #[cfg(not(target_arch = "wasm32"))]
    Native(&'p crate::resolve::record::Native),
}

impl Held<'_> {
    /// What it holds, as its own.
    fn owned(self) -> Owned {
        match self {
            Self::Type(definition) => Owned::Type(Box::new(definition.clone())),
            Self::Method(definition) => Owned::Method(Box::new(definition.clone())),
            Self::Impl(instances) => Owned::Impl(instances.into()),
            Self::Template(key, instances) => Owned::Template(key.clone(), instances.into()),
            Self::Instance(key, types) => Owned::Instance(key.clone(), types.into()),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(native) => Owned::Native(native.clone()),
        }
    }

    /// Whether it holds what `owned` holds.
    fn is(self, owned: &Owned) -> bool {
        match (self, owned) {
            (Self::Type(held), Owned::Type(owned)) => *held == **owned,
            (Self::Method(held), Owned::Method(owned)) => *held == **owned,
            (Self::Impl(held), Owned::Impl(owned)) => *held == **owned,
            (Self::Template(key, held), Owned::Template(owned_key, owned)) => {
                key == owned_key && *held == **owned
            }
            (Self::Instance(key, held), Owned::Instance(owned_key, owned)) => {
                key == owned_key && *held == **owned
            }
            #[cfg(not(target_arch = "wasm32"))]
            (Self::Native(held), Owned::Native(owned)) => held == owned,
            _ => false,
        }
    }
}

/// What a [`Held`] holds, kept with a digest of it (`Digests`).
enum Owned {
    Type(Box<crate::type_def::Definition>),
    Method(Box<crate::method::Definition>),
    Impl(Box<[crate::resolve::ImplInstance]>),
    Template(crate::item::Key, Box<[crate::resolve::Instance]>),
    Instance(
        crate::item::Key,
        Box<[(crate::ImmutableString, type_ref::Type)]>,
    ),
    #[cfg(not(target_arch = "wasm32"))]
    Native(crate::resolve::record::Native),
}

/// One item an interface digests, with what its digest is written from
/// besides: its source's identity and outline, and what the program holds
/// of it.
#[derive(Clone, Copy)]
struct Item<'a> {
    /// The key of its source's outline (`outlines`), which its places are
    /// counted in.
    outline: Option<[u8; 32]>,
    identity: &'a str,
    shell: &'a item::Shell,
    held: &'a [Held<'a>],
}

/// The digests of the items an interface digested, each with what it was
/// digested from, so the next interface, a reveal round's or a later
/// save's, digests again only the items that changed (#1878). An item's
/// digest is written from its source's identity, its places in its
/// source's outline, its shell and what the program holds of it: an item
/// equal in each has the same digest.
pub(super) struct Digests {
    /// What the last interface digested, by its source's outline and its
    /// span.
    kept: BTreeMap<([u8; 32], Span), Vec<Digested>>,
    /// What the interface before it digested, which this one reuses.
    earlier: BTreeMap<([u8; 32], Span), Vec<Digested>>,
}

/// One item's digest, and what it was digested from.
struct Digested {
    identity: Box<str>,
    shell: item::Shell,
    held: Box<[Owned]>,
    digest: [u8; 32],
}

impl Digested {
    /// Whether it is the digest of `item`.
    fn of(&self, item: Item<'_>) -> bool {
        *self.identity == *item.identity
            && self.shell == *item.shell
            && self.held.len() == item.held.len()
            && item
                .held
                .iter()
                .zip(&self.held)
                .all(|(held, owned)| held.is(owned))
    }
}

impl Digests {
    /// None digested yet.
    pub(super) const fn new() -> Self {
        Self {
            kept: BTreeMap::new(),
            earlier: BTreeMap::new(),
        }
    }

    /// Begin an interface: it reuses what the last one digested.
    fn begin(&mut self) {
        self.earlier = std::mem::take(&mut self.kept);
    }

    /// The digest of `item`: the last interface's when it digested the
    /// item as it is, else `digest`'s.
    fn digest(
        &mut self,
        item: Item<'_>,
        digest: impl FnOnce() -> Option<[u8; 32]>,
    ) -> Option<[u8; 32]> {
        let Some(outline) = item.outline else {
            return digest();
        };
        let place = (outline, item.shell.span);
        let earlier = self.earlier.get_mut(&place).and_then(|earlier| {
            let at = earlier.iter().position(|digested| digested.of(item))?;
            Some(earlier.swap_remove(at))
        });
        let digested = match earlier {
            Some(digested) => digested,
            None => Digested {
                identity: item.identity.into(),
                shell: item.shell.clone(),
                held: item.held.iter().map(|held| held.owned()).collect(),
                digest: digest()?,
            },
        };
        let found = digested.digest;
        self.kept.entry(place).or_default().push(digested);
        Some(found)
    }
}

/// The items of each source by where they start, to find the one a place
/// is within.
struct Within(BTreeMap<source::Id, Vec<(Position, Position, usize)>>);

impl Within {
    fn new(items: &[&item::Shell]) -> Self {
        let mut spans = BTreeMap::<source::Id, Vec<_>>::new();
        for (index, shell) in items.iter().enumerate() {
            spans.entry(shell.source).or_default().push((
                shell.span.start(),
                shell.span.end(),
                index,
            ));
        }
        for spans in spans.values_mut() {
            spans.sort_unstable();
        }
        Self(spans)
    }

    /// The innermost item of `source` that `at` is within.
    fn item(&self, source: source::Id, at: Position) -> Option<usize> {
        let spans = self.0.get(&source)?;
        let after = spans.partition_point(|(start, ..)| *start <= at);
        spans[..after]
            .iter()
            .rev()
            .find(|(_, end, _)| at <= *end)
            .map(|(.., index)| *index)
    }
}

/// The names `kind` declares, which a text that writes one reaches it by:
/// none for an impl of a type parameter, which every text reaches.
fn declared_names(kind: &item::Kind) -> Vec<&str> {
    match kind {
        item::Kind::Use(declaration) => declaration
            .bindings
            .iter()
            .map(|binding| binding.name.text.as_str())
            .collect(),
        item::Kind::Struct(declaration) => vec![declaration.name.text.as_str()],
        item::Kind::Enum(declaration) => vec![declaration.name.text.as_str()],
        item::Kind::Function(function) => vec![function.name.text.as_str()],
        item::Kind::Trait(declaration) => std::iter::once(&declaration.name)
            .chain(declaration.functions.iter().map(|function| &function.name))
            .chain(&declaration.associated)
            .map(|name| name.text.as_str())
            .collect(),
        item::Kind::Impl(implementation) => {
            let mut names = Vec::new();
            type_names(&implementation.owner, &mut names);
            if names.is_empty() {
                return names;
            }
            match &implementation.trait_ {
                Some(item::TraitImpl::User { name, .. } | item::TraitImpl::Native { name, .. }) => {
                    names.push(last(&name.text));
                }
                None => {}
            }
            names
        }
    }
}

/// The names of the types `ty` writes, a type parameter's aside.
fn type_names<'t>(ty: &'t type_ref::Type, names: &mut Vec<&'t str>) {
    match ty {
        type_ref::Type::Unit(_) => {}
        type_ref::Type::Tuple { members, .. } => {
            for member in members {
                type_names(member, names);
            }
        }
        type_ref::Type::Reference { target, .. } => type_names(target, names),
        type_ref::Type::Path(path) => {
            match &path.target {
                type_ref::Target::Parameter => {}
                type_ref::Target::Script {
                    identity: crate::type_def::Identity::Script(key),
                    ..
                } => names.push(key.name()),
                _ => names.push(last(&path.name)),
            }
            for argument in &path.arguments {
                if let type_ref::Argument::Type(argument) = argument {
                    type_names(argument, names);
                }
            }
        }
    }
}

/// The last segment of the path `name`.
fn last(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// The digest of what `write` writes, each position written by `places`
/// when there are some and each item and method id unnumbered; `None` when
/// it can't be written.
fn digest(
    places: Option<&Places>,
    write: impl FnOnce(&mut Written) -> std::fmt::Result,
) -> Option<[u8; 32]> {
    let mut written = Written(Sha256::new());
    crate::item::unnumbered(|| match places {
        Some(places) => places.writing(|| write(&mut written)),
        None => write(&mut written),
    })
    .ok()?;
    Some(written.0.finalize().into())
}

/// What another source's lowering reads of `kind`: it, but for its
/// functions' and its impls' methods' bodies' tokens and its inert
/// attributes (`crate::item::inert`). A body's spans and its text when
/// Rawr wrote it stay. A trait's provided bodies stay, as each impl that
/// doesn't override one receives it.
fn outlined(kind: &item::Kind) -> item::Kind {
    let mut kind = kind.clone();
    match &mut kind {
        item::Kind::Use(declaration) => meaningful(&mut declaration.header.attributes),
        item::Kind::Struct(declaration) => {
            meaningful(&mut declaration.header.attributes);
            for field in &mut declaration.fields {
                meaningful(&mut field.attributes);
            }
        }
        item::Kind::Enum(declaration) => {
            meaningful(&mut declaration.header.attributes);
            for variant in &mut declaration.variants {
                meaningful(&mut variant.attributes);
                for attributes in &mut variant.field_attributes {
                    meaningful(attributes);
                }
            }
        }
        item::Kind::Impl(implementation) => {
            meaningful(&mut implementation.header.attributes);
            for function in &mut implementation.functions {
                meaningful(&mut function.header.attributes);
                if !function.body.constant {
                    function.body.syntax = crate::item::BodySyntax::default();
                }
            }
        }
        item::Kind::Trait(declaration) => {
            meaningful(&mut declaration.header.attributes);
            for function in &mut declaration.functions {
                meaningful(&mut function.header.attributes);
            }
        }
        item::Kind::Function(function) => {
            meaningful(&mut function.header.attributes);
            if !function.body.constant {
                function.body.syntax = crate::item::BodySyntax::default();
            }
        }
    }
    kind
}

/// `attributes` without the inert ones.
fn meaningful(attributes: &mut Box<[crate::item::Attribute]>) {
    if attributes.iter().any(crate::item::Attribute::inert) {
        *attributes = attributes
            .iter()
            .filter(|attribute| !attribute.inert())
            .cloned()
            .collect();
    }
}

/// The source of each function's and method's body, by its key.
fn callers(program: &Program) -> BTreeMap<&crate::item::Key, source::Id> {
    program
        .items
        .iter()
        .filter_map(|shell| Some((shell.key.as_ref()?, shell.source)))
        .chain(
            program
                .methods
                .iter()
                .chain(&program.builtin_methods)
                .map(|definition| (definition.key(), definition.source())),
        )
        .collect()
}

/// What only one source's bodies read, by the source: the instance each
/// call of a template takes, by the source of the body that makes it, and
/// the paths and imports its bodies resolve. A call whose body isn't one
/// of the program's is every source's, under `None`.
fn own(program: &Program) -> Option<BTreeMap<Option<source::Id>, [u8; 32]>> {
    let callers = callers(program);
    let mut written = BTreeMap::<Option<source::Id>, Written>::new();
    for ((caller, span), names) in &program.instances.calls {
        write!(
            of(&mut written, callers.get(caller).copied()),
            "{caller:?}|{span:?}|{names:?}|"
        )
        .ok()?;
    }
    for ((source, span), key) in &program.qualified_functions {
        write!(of(&mut written, Some(*source)), "f{span:?}|{key:?}|").ok()?;
    }
    for ((source, span), owner) in &program.qualified_owners {
        write!(of(&mut written, Some(*source)), "o{span:?}|{owner:?}|").ok()?;
    }
    for ((source, span), owner) in &program.qualified_pattern_owners {
        write!(of(&mut written, Some(*source)), "p{span:?}|{owner:?}|").ok()?;
    }
    for ((source, span), path) in &program.qualified_rust {
        write!(of(&mut written, Some(*source)), "r{span:?}|{path:?}|").ok()?;
    }
    for ((source, span), name) in &program.qualified_traits {
        write!(of(&mut written, Some(*source)), "t{span:?}|{name:?}|").ok()?;
    }
    for (source, imports) in &program.lexical {
        write!(of(&mut written, Some(*source)), "l{imports:?}|").ok()?;
    }
    Some(
        written
            .into_iter()
            .map(|(source, written)| (source, written.0.finalize().into()))
            .collect(),
    )
}

/// The digest among `written` of what only `source`'s bodies read.
fn of(
    written: &mut BTreeMap<Option<source::Id>, Written>,
    source: Option<source::Id>,
) -> &mut Written {
    written
        .entry(source)
        .or_insert_with(|| Written(Sha256::new()))
}

/// `text`, after its length, so two texts never run together.
pub(super) fn framed(digest: &mut Sha256, text: &str) {
    digest.update(text.len().to_le_bytes());
    digest.update(text.as_bytes());
}

/// A digest written to as text, as `Debug` writes a value.
pub(super) struct Written(pub(super) Sha256);

impl std::fmt::Write for Written {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.update(text.as_bytes());
        Ok(())
    }
}

/// What another source's lowering reads of one source's text (#1646): its
/// tokens outside its function and method bodies and its inert attributes
/// (`item::inert`), where each of them starts, which its positions are
/// written by, and the names its whole text writes, which reach the
/// declarations its lowering reads (#1718).
#[derive(Clone)]
pub(super) struct Outline {
    /// What it is the outline of: its text and bodies (`outlines`).
    key: [u8; 32],
    tokens: [u8; 32],
    places: Places,
    /// Each name its tokens write, with where it starts, but the names a
    /// declaration gives (a function, a field, a parameter).
    identifiers: crate::Shared<[(Box<str>, Position)]>,
    /// Each name its whole text writes, its bodies' too, once.
    mentioned: crate::Shared<[Box<str>]>,
}

impl Outline {
    /// The outline of `text`, whose function and method bodies are
    /// `bodies`, in order, which `key` digests.
    fn new(engine: &Engine, text: &str, bodies: &[Span], key: [u8; 32]) -> Self {
        // A body within another body is the outer body's.
        let mut merged = Vec::<(Position, Position)>::with_capacity(bodies.len());
        for (start, end) in bodies
            .iter()
            .filter(|span| !span.is_none())
            .map(|span| (span.start(), span.end()))
        {
            match merged.last_mut() {
                Some((_, last)) if start <= *last => *last = (*last).max(end),
                _ => merged.push((start, end)),
            }
        }
        let tokens = super::lex_rust(engine, text);
        let mut starts = Vec::new();
        let mut identifiers = Vec::new();
        let mut mentioned = Vec::new();
        let mut written = Written(Sha256::new());
        let mut next = 0;
        let mut index = 0;
        while let Some((token, position)) = tokens.get(index) {
            if *token == Token::EOF {
                break;
            }
            if let Token::Identifier(name) = token {
                mentioned.push(Box::<str>::from(name.as_str()));
            }
            while merged.get(next).is_some_and(|(_, end)| end <= position) {
                next += 1;
            }
            if merged.get(next).is_some_and(|(start, _)| start <= position) {
                index += 1;
                continue;
            }
            if let Some(length) = inert_attribute(&tokens[index..]) {
                index += length;
                continue;
            }
            // A token's `Debug` can't fail to write.
            let _ = write!(written, "{token:?}|");
            starts.push(*position);
            // A name a declaration gives, a function's, a field's, a
            // parameter's or a generic parameter's, names nothing it
            // reads (#1725).
            let given = index
                .checked_sub(1)
                .is_some_and(|before| matches!(tokens[before].0, Token::Fn))
                || matches!(tokens.get(index + 1), Some((Token::Colon, _)));
            if let Token::Identifier(name) = token
                && !given
            {
                identifiers.push((Box::<str>::from(name.as_str()), *position));
            }
            index += 1;
        }
        mentioned.sort_unstable();
        mentioned.dedup();
        Self {
            key,
            tokens: written.0.finalize().into(),
            places: Places::new(starts),
            identifiers: identifiers.into(),
            mentioned: mentioned.into(),
        }
    }

    /// The names its tokens within `span` write.
    fn identifiers(&self, span: Span) -> impl Iterator<Item = &str> {
        let from = self
            .identifiers
            .partition_point(|(_, at)| *at < span.start());
        let to = self
            .identifiers
            .partition_point(|(_, at)| *at <= span.end())
            .max(from);
        self.identifiers[from..to]
            .iter()
            .map(|(name, _)| name.as_ref())
    }
}

/// How many tokens the inert attribute `tokens` starts with spans, through
/// its `]` (`item::inert`); `None` when they start no inert attribute.
fn inert_attribute(tokens: &[(Token, Position)]) -> Option<usize> {
    let mut rest = tokens.iter().map(|(token, _)| token).enumerate();
    let (_, Token::Hash) = rest.next()? else {
        return None;
    };
    let mut next = rest.next()?;
    if matches!(next, (_, Token::Bang)) {
        next = rest.next()?;
    }
    let (_, Token::LeftBracket) = next else {
        return None;
    };
    let mut path = Vec::new();
    let mut depth = 1_usize;
    let mut reading = true;
    for (index, token) in rest {
        match token {
            Token::Identifier(name) if reading => path.push(name.as_str()),
            Token::DoubleColon if reading && !path.is_empty() => {}
            Token::LeftBracket => {
                reading = false;
                depth += 1;
            }
            Token::RightBracket if depth == 1 => {
                return crate::item::inert(path.into_iter()).then_some(index + 1);
            }
            Token::RightBracket => depth -= 1,
            Token::EOF => return None,
            _ => reading = false,
        }
    }
    None
}

/// The outlines a compiler wrote, by their texts and bodies: a text or
/// bodies that didn't change aren't outlined again.
pub(super) type Outlined = BTreeMap<[u8; 32], Outline>;

/// The outlines of the program's sources and scripts, those `outlined`
/// keeps reused, and `outlined` then the outlines this program has.
fn outlines(engine: &Engine, program: &Program, outlined: &mut Outlined) -> Outlines {
    let mut bodies = BTreeMap::<source::Id, Vec<Span>>::new();
    for shell in program.items.iter() {
        if let item::Kind::Function(function) = &shell.kind
            && !function.body.constant
            && function.body.text.is_none()
        {
            bodies
                .entry(shell.source)
                .or_default()
                .push(function.body.span);
        }
    }
    for definition in program.methods.iter().chain(&program.builtin_methods) {
        if !definition.constant() && definition.body_text().is_none() {
            bodies
                .entry(definition.source())
                .or_default()
                .push(definition.body());
        }
    }
    // An instance's body is its template's, in its template's source.
    for spans in bodies.values_mut() {
        spans.sort_unstable();
        spans.dedup();
    }
    let mut earlier = std::mem::take(outlined);
    let mut outline = |text: &str, spans: &[Span]| {
        let mut written = Written(Sha256::new());
        framed(&mut written.0, text);
        // A span's `Debug` can't fail to write.
        let _ = write!(written, "{spans:?}");
        let key: [u8; 32] = written.0.finalize().into();
        let outline = earlier
            .remove(&key)
            .or_else(|| outlined.get(&key).cloned())
            .unwrap_or_else(|| Outline::new(engine, text, spans, key));
        outlined.insert(key, outline.clone());
        outline
    };
    let sources = program
        .source
        .texts()
        .enumerate()
        .map(|(index, text)| {
            let spans = bodies
                .get(&source::Id::from_index(index))
                .map_or(&[][..], Vec::as_slice);
            outline(text, spans)
        })
        .collect();
    let scripts = program
        .scripts
        .iter()
        .map(|script| outline(&script.text, &[]))
        .collect();
    Outlines { sources, scripts }
}

/// The outlines of a program's sources and of their scripts, each by its
/// index.
struct Outlines {
    sources: Vec<Outline>,
    scripts: Vec<Outline>,
}

#[cfg(test)]
mod tests {
    use super::{Digests, Interface, Models, Native, Outlined};
    use crate::program::source;
    use crate::Engine;

    const SOURCE: &str = "struct State { title: String }\n\
        impl State { fn title(&self) -> &str { &self.title } }\n\
        fn main() { let state = State { title: String::new() }; state.title(); }\n";

    /// What the root's lowering reads of `text`'s program, its items'
    /// digests reused from `digests` where they are as they were.
    fn read(engine: &Engine, text: &str, digests: &mut Digests) -> [u8; 32] {
        let program =
            crate::resolve::resolve(engine.parse_unit(text).unwrap(), &engine.host_registry)
                .unwrap();
        let runtime =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), &program);
        let models = Models::new(&program, runtime, engine.host_registry.share(), engine);
        let native = Native::new(&program, &models.host);
        Interface::new(
            engine,
            &program,
            &models,
            &mut Outlined::new(),
            digests,
            &native,
        )
        .unwrap()
        .of(source::Id::from_index(0))
    }

    /// An item's digest an interface reuses is the one it would write
    /// again, and one whose item changed is written again (#1878).
    #[test]
    fn an_interface_reuses_only_the_digests_of_items_as_they_were() {
        let engine = Engine::new();
        let mut digests = Digests::new();
        let first = read(&engine, SOURCE, &mut digests);
        assert_eq!(read(&engine, SOURCE, &mut digests), first);
        let changed = SOURCE.replace("title: String }", "title: String, visits: u32 }");
        let changed = changed.replace("String::new() }", "String::new(), visits: 0 }");
        let after = read(&engine, &changed, &mut digests);
        assert_ne!(after, first);
        assert_eq!(after, read(&engine, &changed, &mut Digests::new()));
        assert_eq!(read(&engine, SOURCE, &mut digests), first);
    }
}
