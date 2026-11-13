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
//! A name reaches only the declarations it can name (#1886, `Scopes`): a
//! name a source writes alone names its own module's items, what its
//! `use`s import and what its glob `use`s bring in; one below a path
//! names what the path's first name leads to. So a widget's own `State`
//! never reaches a screen's, and a field the screen's gains keeps the
//! widget's key. A path through `crate`, `self`, `super`, a module or a
//! name the source doesn't know may lead to any module's; one through a
//! crate of the extern prelude reaches only what any name reaches: an
//! impl of another module's or a crate's type, a `macro_rules!`, a
//! mirror. A facade entry whose paths are in one source's module is that
//! source's.
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
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

/// What each source's lowering reads of the program, the other sources'
/// bodies aside.
pub(super) struct Interface {
    /// What every source's lowering reads.
    every: [u8; 32],
    /// What reveal's reading of every body reads.
    every_revealed: [u8; 32],
    /// What each source's lowering reads, `every` among it.
    sources: BTreeMap<source::Id, [u8; 32]>,
    /// What reveal's reading of each source's bodies reads, `every` among
    /// it, but for the instance each body's calls take, which a reading's
    /// key writes body by body (`Calls`), and the private fields of
    /// another module's struct, which its bodies can't name
    /// (`Declaration::outside`).
    revealed: BTreeMap<source::Id, [u8; 32]>,
}

impl Interface {
    /// What `source`'s lowering reads of the program.
    pub(super) fn of(&self, source: source::Id) -> [u8; 32] {
        self.sources.get(&source).copied().unwrap_or(self.every)
    }

    /// What reveal's reading of a body of `source` reads of the program,
    /// but for the instances its calls take (`Calls`).
    pub(super) fn revealed(&self, source: source::Id) -> [u8; 32] {
        self.revealed
            .get(&source)
            .copied()
            .unwrap_or(self.every_revealed)
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
        let scopes = Scopes::new(program, &outlines, native);
        let declarations = Declarations::new(program, &outlines, &scopes, native, digests)?;
        let own = own(program)?;
        let calls = Calls::by_source(program)?;
        // A runner's compiler reads the facades registered in the process
        // as its image holds them; a check reads its own registry's alone,
        // which the facades below write whole, so another engine's
        // registration keeps every source's key (#1850).
        let registered = engine
            .imported_native
            .is_some()
            .then(Engine::registered_facades);
        let compiler = every(
            program,
            registered,
            models,
            &outlines,
            [own.get(&None), calls.get(&None)],
            native,
        )?;
        let every = with(compiler, declarations.everywhere);
        let every_revealed = with(compiler, declarations.revealed);
        let namespaces = program
            .scripts
            .iter()
            .map(|script| (script.source, &script.namespace))
            .collect::<BTreeMap<_, _>>();
        let mut sources = BTreeMap::new();
        let mut revealed = BTreeMap::new();
        let mut walk = Walk::new(&declarations);
        for (index, outline) in outlines.sources.iter().enumerate() {
            let source = source::Id::from_index(index);
            // What the program resolved of the source's own names.
            let mut written = Written(Sha256::new());
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
            let resolved: [u8; 32] = written.0.finalize().into();
            let mut lowering = Sha256::new();
            lowering.update(every);
            declarations.reached(
                source,
                &scopes,
                &outline.mentioned,
                &mut walk,
                &mut lowering,
                None,
            );
            lowering.update(resolved);
            lowering.update(calls.get(&Some(source)).unwrap_or(&[0; 32]));
            sources.insert(source, lowering.finalize().into());
            // Another module's struct as the source's bodies read it: its
            // private fields aside, unless the source is that module or
            // below it.
            let here = namespaces.get(&source);
            let outside = |declared: source::Id| {
                declared != source
                    && !here
                        .zip(namespaces.get(&declared))
                        .is_some_and(|(here, declared)| here.sees_private(declared))
            };
            let mut reading = Sha256::new();
            reading.update(every_revealed);
            declarations.reached(
                source,
                &scopes,
                &outline.mentioned,
                &mut walk,
                &mut reading,
                Some(&outside),
            );
            reading.update(resolved);
            revealed.insert(source, reading.finalize().into());
        }
        Some(Self {
            every,
            every_revealed,
            sources,
            revealed,
        })
    }
}

/// What each template's instances are, by the template's key: each
/// instance's types and its signature, the output a reveal gives it among
/// it. A reading of a body is reused only while each template its calls
/// bind is as it was then (#1755).
pub(super) struct Instances(BTreeMap<crate::item::Key, [u8; 32]>);

impl Instances {
    /// Each template's instances in `program`.
    pub(super) fn new(program: &Program) -> Option<Self> {
        let functions = program
            .items
            .iter()
            .filter_map(|shell| match &shell.kind {
                item::Kind::Function(definition) => Some((shell.key.as_ref()?, definition)),
                _ => None,
            })
            .filter(|(key, _)| program.instances.types.contains_key(*key))
            .collect::<BTreeMap<_, _>>();
        let methods = program
            .methods
            .iter()
            .chain(&program.builtin_methods)
            .filter(|definition| program.instances.types.contains_key(definition.key()))
            .map(|definition| (definition.key(), definition))
            .collect::<BTreeMap<_, _>>();
        let mut templates = BTreeMap::new();
        for (template, instances) in &program.instances.templates {
            let digest = digest(None, |written| {
                for (names, key) in instances {
                    write!(written, "{names:?}|{key:?}|")?;
                    if let Some(definition) = functions.get(key) {
                        write!(written, "{definition:?}|")?;
                    }
                    if let Some(definition) = methods.get(key) {
                        write!(written, "{definition:?}|")?;
                    }
                }
                Ok(())
            })?;
            templates.insert(template.clone(), digest);
        }
        Some(Self(templates))
    }

    /// The digest of the instances of the template `key`; `None` when it
    /// is none.
    pub(super) fn of(&self, key: &crate::item::Key) -> Option<[u8; 32]> {
        self.0.get(key).copied()
    }
}

/// The instance each call of a template takes (`resolve::Instances::calls`),
/// digested by the body that makes the call: a body reads only its own
/// calls' (`Parser::called_instance`), so reveal reads a body again only
/// when its own calls' change, not another body's (#1755).
pub(super) struct Calls(BTreeMap<crate::item::Key, [u8; 32]>);

impl Calls {
    /// Each body's calls in `program`.
    pub(super) fn new(program: &Program) -> Option<Self> {
        let mut written = BTreeMap::<&crate::item::Key, Written>::new();
        for ((caller, span), names) in &program.instances.calls {
            let written = written
                .entry(caller)
                .or_insert_with(|| Written(Sha256::new()));
            write!(written, "{span:?}|{names:?}|").ok()?;
        }
        Some(Self(
            written
                .into_iter()
                .map(|(caller, written)| (caller.clone(), written.0.finalize().into()))
                .collect(),
        ))
    }

    /// The digest of the calls the body `key` makes.
    pub(super) fn of(&self, key: &crate::item::Key) -> [u8; 32] {
        self.0.get(key).copied().unwrap_or([0; 32])
    }

    /// Each source's bodies' calls, by the source of the body that makes
    /// them; a call whose body isn't one of the program's is every
    /// source's, under `None`.
    fn by_source(program: &Program) -> Option<BTreeMap<Option<source::Id>, [u8; 32]>> {
        let callers = callers(program);
        let mut written = BTreeMap::<Option<source::Id>, Written>::new();
        for ((caller, span), names) in &program.instances.calls {
            write!(
                of(&mut written, callers.get(caller).copied()),
                "{caller:?}|{span:?}|{names:?}|"
            )
            .ok()?;
        }
        Some(
            written
                .into_iter()
                .map(|(source, written)| (source, written.0.finalize().into()))
                .collect(),
        )
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
    own: [Option<&[u8; 32]>; 2],
    native: &Native,
) -> Option<[u8; 32]> {
    let mut written = Written(Sha256::new());
    written.0.update(b"rawr.lowering.interface/v12");
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
    for own in own {
        written.0.update(own.unwrap_or(&[0; 32]));
    }
    Some(written.0.finalize().into())
}

/// `compiler`, what every source reads of the compiler and the program, with
/// `declarations`, what it reads of the declarations no name reaches.
fn with(compiler: [u8; 32], declarations: [u8; 32]) -> [u8; 32] {
    let mut written = Sha256::new();
    written.update(compiler);
    written.update(declarations);
    written.finalize().into()
}

/// Which reader a declaration's digest is written for.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Reader {
    /// A source's lowering.
    Lowering,
    /// Reveal's reading of a body (`Digest::revealed`).
    Reveal,
}

/// Write each of `held` into `written`; for reveal, a
/// template's key alone, its instances aside, and no instance method or
/// instance's types, which a reading of a body checks against the
/// templates its own calls bind (`Instances`).
fn write_held(
    held: &[Held<'_>],
    reader: Reader,
    instances: &crate::resolve::Instances,
    written: &mut Written,
) -> std::fmt::Result {
    for held in held {
        match held {
            Held::Template(key, _) if reader == Reader::Reveal => {
                write!(written, "Template({key:?})|")?;
            }
            held if reader == Reader::Reveal && held.instanced(instances) => {}
            held => write!(written, "{held:?}|")?,
        }
    }
    Ok(())
}

/// The program's declarations, each digested on its own, by the names that
/// reach them.
struct Declarations<'p> {
    /// Each name some declaration declares, by its number.
    names: HashMap<&'p str, usize>,
    /// The declarations that declare each name, by its number, each with
    /// which texts' names reach it (`Declared`).
    declaring: Vec<Vec<(usize, Declared)>>,
    /// Each source with each declared name its own items declare, which a
    /// `Declared::Native` entry of that name may be the native side of.
    declares: HashSet<(source::Id, usize)>,
    declarations: Vec<Declaration>,
    /// What every source reads of them: the impls of a type parameter, and
    /// what the program holds of no item.
    everywhere: [u8; 32],
    /// What reveal's reading of every body reads of them (`Digest::
    /// revealed`).
    revealed: [u8; 32],
}

/// A declaration's digests: as a source's lowering reads it, and as
/// reveal's reading of a body reads it (#1755).
#[derive(Clone, Copy)]
struct Digest {
    lowering: [u8; 32],
    /// As reveal reads it, when that differs: the instances a template
    /// has aside, and its impl's instance methods, which a reading checks
    /// against the templates its own calls bind (`Instances`).
    revealed: Option<[u8; 32]>,
    /// As reveal reads a struct with a private field in another module,
    /// whose bodies can't name the field: without it.
    outside: Option<[u8; 32]>,
}

impl Digest {
    /// A declaration reveal reads as a lowering does.
    const fn plain(lowering: [u8; 32]) -> Self {
        Self {
            lowering,
            revealed: None,
            outside: None,
        }
    }
}

/// One declaration: an item and what the program resolved of it.
struct Declaration {
    digest: Digest,
    /// The source that declares it, whose module and those below it see
    /// its private fields.
    source: Option<source::Id>,
    /// The numbers of the declared names its declaration writes, which a
    /// source that reaches it reaches too, each as its text names it
    /// (`By`).
    writes: Vec<(usize, By)>,
}

/// Which texts' names reach a declaration that declares them (#1886).
#[derive(Clone, Copy)]
enum Declared {
    /// A name its source writes alone, or one any text names through a
    /// path or an import that may lead there: an item, or an impl of its
    /// source's own type.
    In(source::Id),
    /// An entry of a facade outside every source's module: a name a source
    /// writes alone reaches it only when one of its own items declares the
    /// name, as the entry may be that item's native side.
    Native,
    /// Any name: an impl of another module's or a crate's type or trait, a
    /// `macro_rules!`, which a call names in any module, a mirror, or an
    /// impl entry of a facade.
    Any,
}

/// How a text names a declared name (`Scopes::reach`).
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum By {
    /// Through a path or an import that may lead to any module.
    Open,
    /// Alone in this source, or through a path that stays in it.
    Local(source::Id),
    /// Through a crate of the extern prelude, the program's own and its
    /// path crates aside: it reaches only what any name reaches.
    Foreign,
}

impl Declared {
    /// Whether a name written as `by` reaches a declaration of it declared
    /// so; `declares` says whether a source's own items declare it.
    fn reached(self, by: By, declares: impl FnOnce(source::Id) -> bool) -> bool {
        match (by, self) {
            (By::Open, _) | (_, Self::Any) => true,
            (By::Local(source), Self::In(declared)) => source == declared,
            (By::Local(source), Self::Native) => declares(source),
            (By::Foreign, _) => false,
        }
    }
}

impl<'p> Declarations<'p> {
    /// The program's declarations, whose sources' outlines are `outlines`,
    /// and the entries of its own facades, `native`'s. An item `digests`
    /// digested as it is now isn't digested again.
    fn new(
        program: &'p Program,
        outlines: &'p Outlines,
        scopes: &Scopes<'p>,
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
        let mut declaring = Vec::<Vec<(usize, Declared)>>::new();
        let mut everywhere = Written(Sha256::new());
        let mut revealed = Written(Sha256::new());
        let mut unnamed = Vec::with_capacity(items.len());
        for (index, shell) in items.iter().enumerate() {
            let named = declared_names(&shell.kind, shell.source, scopes.namespace(shell.source));
            for (name, declared) in &named {
                let number = *names.entry(name).or_insert_with(|| {
                    declaring.push(Vec::new());
                    declaring.len() - 1
                });
                declaring[number].push((index, *declared));
            }
            unnamed.push(named.is_empty());
        }
        // Each entry of the program's own facades, after the items. One no
        // name reaches is in what every source reads of them.
        for (offset, entry) in native.entries().iter().enumerate() {
            let declared = scopes.entry(entry);
            for name in entry.names() {
                let number = *names.entry(name).or_insert_with(|| {
                    declaring.push(Vec::new());
                    declaring.len() - 1
                });
                declaring[number].push((items.len() + offset, declared));
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
                declaring[number].push((mirrors + offset, Declared::Any));
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
            declaring[number].push((macros + offset, Declared::Any));
        }
        let declares = scopes.declares(&names);
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
            let found = digests.digest(item, || {
                let places = outline
                    .map(|outline| outline.places.between(shell.span.start(), shell.span.end()));
                let item_digest = |reader: Reader| {
                    digest(places.as_ref(), |written| {
                        let kind = outlined(&shell.kind);
                        write!(
                            written,
                            "{identity}|{:?}|{:?}|{:?}|{kind:?}|",
                            shell.key, shell.span, shell.facades,
                        )?;
                        write_held(held, reader, &program.instances, written)
                    })
                };
                let lowering = item_digest(Reader::Lowering)?;
                let revealed = match held.iter().any(|held| held.instanced(&program.instances)) {
                    true => Some(item_digest(Reader::Reveal)?),
                    false => None,
                };
                let outside = match outline {
                    Some(outline) => outside(item, outline, places.as_ref(), &program.instances)?,
                    None => None,
                };
                Some(Digest {
                    lowering,
                    revealed,
                    outside,
                })
            })?;
            let writes = scopes.writes(
                shell.source,
                outline
                    .into_iter()
                    .flat_map(|outline| outline.identifiers(shell.span)),
                &names,
            );
            if unnamed[index] {
                everywhere.0.update(found.lowering);
                revealed.0.update(found.revealed.unwrap_or(found.lowering));
            }
            declarations.push(Declaration {
                digest: found,
                source: Some(shell.source),
                writes,
            });
        }
        for entry in native.entries() {
            let writes = open(entry.writes(), &names);
            declarations.push(Declaration {
                digest: Digest::plain(*entry.digest()),
                source: None,
                writes,
            });
        }
        for mirror in &program.mirrors {
            declarations.push(Declaration {
                digest: Digest::plain(digest(None, |written| write!(written, "{mirror:?}"))?),
                source: None,
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
            // A call expands it in its own module: what it writes may name
            // any module's.
            let writes = open(
                outline
                    .into_iter()
                    .flat_map(|outline| outline.identifiers(span))
                    .map(|(name, _)| name),
                &names,
            );
            declarations.push(Declaration {
                digest: Digest::plain(digest),
                source: Some(definition.source()),
                writes,
            });
        }
        everywhere.0.update(digest(None, |written| {
            write_held(&unheld, Reader::Lowering, &program.instances, written)
        })?);
        revealed.0.update(digest(None, |written| {
            write_held(&unheld, Reader::Reveal, &program.instances, written)
        })?);
        Some(Self {
            names,
            declaring,
            declares,
            declarations,
            everywhere: everywhere.0.finalize().into(),
            revealed: revealed.0.finalize().into(),
        })
    }

    /// Write into `digest` the declarations a text that writes the names
    /// `mentioned` reaches: how many, and their digests summed lane by
    /// lane, which no order of the walk changes. The walk touches only
    /// what it reaches, and no digest is digested again (#1850). Reveal's
    /// reading of a body reads each as `Digest::revealed`, and a struct a
    /// source `outside` says the text's module doesn't see the private
    /// items of as `Digest::outside`.
    fn reached(
        &self,
        source: source::Id,
        scopes: &Scopes<'_>,
        mentioned: &[(Box<str>, Head)],
        walk: &mut Walk,
        digest: &mut Sha256,
        reveal: Option<&dyn Fn(source::Id) -> bool>,
    ) {
        let mut sum = [0_u64; 4];
        let mut count = 0_u64;
        for (name, head) in mentioned {
            if let Some(&number) = self.names.get(name.as_ref()) {
                walk.see(number, scopes.reach(source, name, head).by(source));
            }
        }
        while let Some((name, by)) = walk.next.pop() {
            for &(index, declared) in &self.declaring[name] {
                if walk.reached[index]
                    || !declared.reached(by, |source| self.declares.contains(&(source, name)))
                {
                    continue;
                }
                walk.reached[index] = true;
                walk.found.push(index);
                count += 1;
                let declaration = &self.declarations[index];
                let found = declaration.digest;
                let read = match reveal {
                    None => found.lowering,
                    Some(outside) => found
                        .outside
                        .filter(|_| declaration.source.is_some_and(outside))
                        .or(found.revealed)
                        .unwrap_or(found.lowering),
                };
                let (lanes, _) = read.as_chunks::<8>();
                for (lane, bytes) in sum.iter_mut().zip(lanes) {
                    *lane = lane.wrapping_add(u64::from_le_bytes(*bytes));
                }
                for &(written, by) in &self.declarations[index].writes {
                    walk.see(written, by);
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
    /// Each declared name the walk saw, by its number, with how.
    seen: HashSet<(usize, By)>,
    /// Whether it reached each declaration, by its number.
    reached: Vec<bool>,
    /// The names it has yet to follow.
    next: Vec<(usize, By)>,
    /// The declarations it reached.
    found: Vec<usize>,
}

impl Walk {
    fn new(declarations: &Declarations<'_>) -> Self {
        Self {
            seen: HashSet::new(),
            reached: vec![false; declarations.declarations.len()],
            next: Vec::new(),
            found: Vec::new(),
        }
    }

    /// Follow the name `name`, written as `by`, next, unless the walk saw
    /// it so.
    fn see(&mut self, name: usize, by: By) {
        if self.seen.insert((name, by)) {
            self.next.push((name, by));
        }
    }

    /// Unmark what the walk marked.
    fn clear(&mut self) {
        self.seen.clear();
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
    /// Whether reveal reads it otherwise than a lowering does
    /// (`write_held`): a template's instances, an instance method, or an
    /// instance's types.
    fn instanced(self, instances: &crate::resolve::Instances) -> bool {
        match self {
            Self::Template(..) | Self::Instance(..) => true,
            Self::Method(definition) => instances.types.contains_key(definition.key()),
            _ => false,
        }
    }

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

/// The digest of `item`, a struct with a private field, as another module
/// reads it: its public fields alone, each written by its own places, so a
/// private field a save adds before one keeps it the same, and its type
/// but for its schema, which its private fields write too. `None` for any
/// other item. Its places are `places`, as its digest's are.
fn outside(
    item: Item<'_>,
    outline: &Outline,
    places: Option<&Places>,
    instances: &crate::resolve::Instances,
) -> Option<Option<[u8; 32]>> {
    let item::Kind::Struct(declared) = &item.shell.kind else {
        return Some(None);
    };
    let public = |field: &item::Field| {
        outline
            .private
            .binary_search(&field.name.span.start())
            .is_err()
    };
    if declared.form != crate::item::StructForm::Named || declared.fields.iter().all(public) {
        return Some(None);
    }
    let mut bare = declared.clone();
    bare.fields = Box::default();
    let kind = outlined(&item::Kind::Struct(bare));
    let identity = item.identity;
    let shell = item.shell;
    let digest = digest(places, |written| {
        write!(
            written,
            "outside|{identity}|{:?}|{:?}|{:?}|{kind:?}|",
            shell.key,
            shell.span.start(),
            shell.facades,
        )?;
        for field in declared.fields.iter().filter(|field| public(field)) {
            let mut field = field.clone();
            meaningful(&mut field.attributes);
            outline
                .places
                .between(field.span.start(), field.span.end())
                .writing(|| write!(written, "{field:?}|"))?;
        }
        for held in item.held {
            match held {
                Held::Type(definition) => {
                    definition.write_without_schema(written)?;
                    write!(written, "|")?;
                }
                held => write_held(
                    std::slice::from_ref(held),
                    Reader::Reveal,
                    instances,
                    written,
                )?,
            }
        }
        Ok(())
    })?;
    Some(Some(digest))
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
    digest: Digest,
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
        digest: impl FnOnce() -> Option<Digest>,
    ) -> Option<Digest> {
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

/// The names `kind`, an item of `source` in the module `namespace`,
/// declares, which a text that writes one reaches it by, each with which
/// texts' names reach it: none for an impl of a type parameter, which
/// every text reaches. An impl of its own module's type is that module's,
/// as the type is; any other impl is any text's that names its type or
/// its trait.
fn declared_names<'k>(
    kind: &'k item::Kind,
    source: source::Id,
    namespace: Option<&crate::resolve::Namespace>,
) -> Vec<(&'k str, Declared)> {
    let own = |name: &'k str| (name, Declared::In(source));
    match kind {
        item::Kind::Use(declaration) => declaration
            .bindings
            .iter()
            .map(|binding| own(binding.name.text.as_str()))
            .collect(),
        item::Kind::Struct(declaration) => vec![own(declaration.name.text.as_str())],
        item::Kind::Enum(declaration) => vec![own(declaration.name.text.as_str())],
        item::Kind::Function(function) => vec![own(function.name.text.as_str())],
        item::Kind::Trait(declaration) => std::iter::once(&declaration.name)
            .chain(declaration.functions.iter().map(|function| &function.name))
            .chain(&declaration.associated)
            .map(|name| own(name.text.as_str()))
            .collect(),
        item::Kind::Impl(implementation) => {
            let mut types = Vec::new();
            type_names(&implementation.owner, &mut types);
            if types.is_empty() {
                return Vec::new();
            }
            let mut names = types
                .into_iter()
                .map(|(name, key)| {
                    let here = key.zip(namespace).is_some_and(|(key, namespace)| {
                        key.package() == namespace.package.as_str()
                            && key.modules() == &*namespace.modules
                    });
                    (
                        name,
                        if here {
                            Declared::In(source)
                        } else {
                            Declared::Any
                        },
                    )
                })
                .collect::<Vec<_>>();
            match &implementation.trait_ {
                Some(item::TraitImpl::User { name, .. } | item::TraitImpl::Native { name, .. }) => {
                    names.push((last(&name.text), Declared::Any));
                }
                None => {}
            }
            names
        }
    }
}

/// The names of the types `ty` writes, a type parameter's aside, each with
/// the key of the program's type it names, when it names one.
fn type_names<'t>(
    ty: &'t type_ref::Type,
    names: &mut Vec<(&'t str, Option<&'t crate::type_def::Key>)>,
) {
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
                } => names.push((key.name(), Some(key))),
                _ => names.push((last(&path.name), None)),
            }
            for argument in &path.arguments {
                if let type_ref::Argument::Type(argument) = argument {
                    type_names(argument, names);
                }
            }
        }
    }
}

/// The numbers of the declared names among `written`, each named through
/// a path that may lead to any module.
fn open<'n>(
    written: impl Iterator<Item = &'n str>,
    names: &HashMap<&str, usize>,
) -> Vec<(usize, By)> {
    let mut writes = written
        .filter_map(|name| names.get(name).map(|number| (*number, By::Open)))
        .collect::<Vec<_>>();
    writes.sort_unstable_by_key(|(number, _)| *number);
    writes.dedup_by_key(|(number, _)| *number);
    writes
}

/// What a name a text writes can name (#1886): a declaration of its own
/// source, one of any module, or neither, a crate's alone.
#[derive(Clone, Copy, Default)]
struct Reach {
    local: bool,
    open: bool,
}

impl Reach {
    const NONE: Self = Self {
        local: false,
        open: false,
    };
    const LOCAL: Self = Self {
        local: true,
        open: false,
    };
    const OPEN: Self = Self {
        local: false,
        open: true,
    };

    /// How a name `source` writes so names a declared name.
    const fn by(self, source: source::Id) -> By {
        match self {
            Self { open: true, .. } => By::Open,
            Self { local: true, .. } => By::Local(source),
            _ => By::Foreign,
        }
    }
}

impl std::ops::BitOr for Reach {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self {
            local: self.local || other.local,
            open: self.open || other.open,
        }
    }
}

/// What each source's names can name (#1886). A name a source writes
/// alone names its own item, an import, or one a glob `use` brings in; one
/// below a path names what the path's first name leads to. So a name
/// reaches another module's declaration only through a path or an import
/// that may lead there, and a widget's `State` never reaches a screen's.
/// Each test errs toward reaching more: a path whose start the text
/// doesn't spell, `crate`, `self`, `super`, a module, or a name the source
/// doesn't know may lead to any module.
struct Scopes<'p> {
    sources: Vec<Names<'p>>,
    namespaces: BTreeMap<source::Id, &'p crate::resolve::Namespace>,
    /// The crates of the extern prelude, the program's own and its path
    /// crates aside, which name no source's declaration.
    foreign: Option<&'p crate::resolve::extern_prelude::Prelude>,
    /// The program's own crates and its path crates, by the names a path
    /// starts with.
    native: HashSet<String>,
    /// The source of each module, by its path from each of the program's
    /// own crates' roots, as a facade's entries are; `None` for a path two
    /// sources have.
    modules: HashMap<String, Option<source::Id>>,
}

/// What one source's names can name.
struct Names<'p> {
    /// The names its own structs, enums, traits and functions have.
    declared: HashSet<&'p str>,
    /// The modules its `mod`s declare.
    modules: HashSet<&'p str>,
    /// What each name its `use`s write reaches.
    imports: HashMap<&'p str, Reach>,
    /// What its glob `use`s bring in.
    globbed: Reach,
}

impl<'p> Scopes<'p> {
    fn new(program: &'p Program, outlines: &'p Outlines, native: &Native) -> Self {
        let mut declared = vec![HashSet::new(); outlines.sources.len()];
        for shell in program.items.iter() {
            let name = match &shell.kind {
                item::Kind::Struct(declaration) => &declaration.name,
                item::Kind::Enum(declaration) => &declaration.name,
                item::Kind::Trait(declaration) => &declaration.name,
                item::Kind::Function(function) => &function.name,
                item::Kind::Use(_) | item::Kind::Impl(_) => continue,
            };
            if let Some(declared) = declared.get_mut(shell.source.index()) {
                declared.insert(name.text.as_str());
            }
        }
        let crates = program
            .own
            .iter()
            .map(|own| own.as_str())
            .chain(native.roots.iter().map(AsRef::as_ref))
            .map(str::to_owned);
        let roots = program
            .own
            .iter()
            .map(|own| own.replace('-', "_"))
            .collect::<Vec<_>>();
        let mut modules = HashMap::<String, Option<source::Id>>::new();
        for script in &program.scripts {
            for root in &roots {
                let path = std::iter::once(root.as_str())
                    .chain(
                        script
                            .namespace
                            .modules
                            .iter()
                            .map(|module| module.as_str()),
                    )
                    .collect::<Vec<_>>()
                    .join("::");
                let found = modules.entry(path).or_insert(Some(script.source));
                if *found != Some(script.source) {
                    *found = None;
                }
            }
        }
        let mut scopes = Self {
            sources: Vec::with_capacity(outlines.sources.len()),
            namespaces: program
                .scripts
                .iter()
                .map(|script| (script.source, &script.namespace))
                .collect(),
            foreign: program.crates.as_ref(),
            native: crates
                .flat_map(|name| [name.replace('-', "_"), name])
                .collect(),
            modules,
        };
        for (outline, declared) in outlines.sources.iter().zip(declared) {
            let mut names = Names {
                declared,
                modules: outline.modules.iter().map(AsRef::as_ref).collect(),
                imports: HashMap::new(),
                globbed: Reach::NONE,
            };
            for (name, head) in outline.imports.iter() {
                let reach = scopes.start(&names, head, false);
                let entry = names.imports.entry(name).or_default();
                *entry = *entry | reach;
            }
            for head in outline.globs.iter() {
                names.globbed = names.globbed | scopes.start(&names, head, false);
            }
            scopes.sources.push(names);
        }
        scopes
    }

    /// The module of `source`.
    fn namespace(&self, source: source::Id) -> Option<&crate::resolve::Namespace> {
        self.namespaces.get(&source).copied()
    }

    /// What the name `name`, which `source` writes below `head`, can name.
    fn reach(&self, source: source::Id, name: &str, head: &Head) -> Reach {
        let Some(names) = self.sources.get(source.index()) else {
            return Reach::LOCAL | Reach::OPEN;
        };
        match head {
            Head::Bare => {
                let imported = names.imports.get(name).copied().unwrap_or_default();
                Reach::LOCAL | imported | names.globbed
            }
            path => self.start(names, path, true),
        }
    }

    /// What a path that starts below `head` leads to in the source of
    /// `names`, through its imports when `imports` says so.
    fn start(&self, names: &Names<'_>, head: &Head, imports: bool) -> Reach {
        let start = match head {
            Head::Bare => return Reach::LOCAL,
            Head::Unknown => return Reach::LOCAL | Reach::OPEN,
            Head::Path(start) => start.as_ref(),
        };
        match start {
            "crate" | "self" | "super" => return Reach::OPEN,
            "Self" => return Reach::LOCAL,
            _ => {}
        }
        let imported = imports.then(|| names.imports.get(start)).flatten();
        let mut reach = imported.copied().unwrap_or_default();
        if names.declared.contains(start) {
            reach = reach | Reach::LOCAL;
        }
        if names.modules.contains(start) {
            reach = reach | Reach::OPEN;
        }
        let known =
            imported.is_some() || names.declared.contains(start) || names.modules.contains(start);
        if known || self.is_foreign(start) {
            reach
        } else {
            Reach::OPEN
        }
    }

    /// Whether a path that starts with `start` names a crate of the extern
    /// prelude that holds no declaration of the program.
    fn is_foreign(&self, start: &str) -> bool {
        let prelude = match self.foreign {
            Some(prelude) => prelude.holds(start),
            None => matches!(start, "std" | "core"),
        };
        prelude && !self.native.contains(start)
    }

    /// Which texts' names reach `entry`: the source whose module its paths
    /// are in, when one is.
    fn entry(&self, entry: &image::outline::Entry) -> Declared {
        let source = entry
            .module()
            .and_then(|module| self.modules.get(module).copied().flatten());
        match source {
            Some(source) => Declared::In(source),
            None if entry.declared() => Declared::Native,
            None => Declared::Any,
        }
    }

    /// Each source with each of `names` its own items declare.
    fn declares(&self, names: &HashMap<&str, usize>) -> HashSet<(source::Id, usize)> {
        self.sources
            .iter()
            .enumerate()
            .flat_map(|(index, source)| {
                source.declared.iter().filter_map(move |name| {
                    Some((source::Id::from_index(index), *names.get(name)?))
                })
            })
            .collect()
    }

    /// The numbers of the declared names among `written`, which `source`
    /// writes, each as it names it.
    fn writes<'n>(
        &self,
        source: source::Id,
        written: impl Iterator<Item = (&'n str, &'n Head)>,
        names: &HashMap<&str, usize>,
    ) -> Vec<(usize, By)> {
        let mut writes = written
            .filter_map(|(name, head)| {
                let number = *names.get(name)?;
                Some((number, self.reach(source, name, head).by(source)))
            })
            .collect::<Vec<_>>();
        writes.sort_unstable_by_key(|(number, by)| (*number, *by == By::Open));
        writes.dedup();
        writes
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

/// What only one source's bodies read, by the source: the paths and
/// imports its bodies resolve. The instances their calls take are
/// `Calls`'.
fn own(program: &Program) -> Option<BTreeMap<Option<source::Id>, [u8; 32]>> {
    let mut written = BTreeMap::<Option<source::Id>, Written>::new();
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
    /// Each name its tokens write, with where it starts and the path it is
    /// below, but the names a declaration gives (a function, a field, a
    /// parameter).
    identifiers: crate::Shared<[(Box<str>, Position, Head)]>,
    /// Each name its whole text writes, its bodies' too, with the path it
    /// is below, once.
    mentioned: crate::Shared<[(Box<str>, Head)]>,
    /// Each name a `use` writes, with the path the `use` starts with.
    imports: crate::Shared<[(Box<str>, Head)]>,
    /// The path each glob `use` starts with.
    globs: crate::Shared<[Head]>,
    /// Each module a `mod` declares.
    modules: crate::Shared<[Box<str>]>,
    /// Where each name no visibility marks starts, in order: a struct's
    /// private fields' among them (`outside`).
    private: crate::Shared<[Position]>,
}

/// The path a name is below, as a text writes it (#1886): what the name
/// can name follows from it (`Names::reach`).
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Head {
    /// Written alone: `State`.
    Bare,
    /// Below a path that starts with this name: `home` of `home::State`,
    /// or of each name a `use` statement writes.
    Path(Box<str>),
    /// Below a path whose start the text doesn't spell as names:
    /// `Vec::<T>::new`, `<T as Trait>::N`, `::std::mem::take`.
    Unknown,
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
        let private = private_names(&tokens);
        let paths = Paths::new(&tokens);
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
                mentioned.push((Box::<str>::from(name.as_str()), paths.heads[index].clone()));
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
                identifiers.push((
                    Box::<str>::from(name.as_str()),
                    *position,
                    paths.heads[index].clone(),
                ));
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
            imports: paths.imports.into(),
            globs: paths.globs.into(),
            modules: paths.modules.into(),
            private,
        }
    }

    /// The names its tokens within `span` write, each with the path it is
    /// below.
    fn identifiers(&self, span: Span) -> impl Iterator<Item = (&str, &Head)> {
        let from = self
            .identifiers
            .partition_point(|(_, at, _)| *at < span.start());
        let to = self
            .identifiers
            .partition_point(|(_, at, _)| *at <= span.end())
            .max(from);
        self.identifiers[from..to]
            .iter()
            .map(|(name, _, head)| (name.as_ref(), head))
    }
}

/// The paths a text's names are below (`Head`), and what its `use` and
/// `mod` statements write.
struct Paths {
    /// Each token's head, by its index: `Bare` for a token not a name.
    heads: Vec<Head>,
    imports: Vec<(Box<str>, Head)>,
    globs: Vec<Head>,
    modules: Vec<Box<str>>,
}

impl Paths {
    fn new(tokens: &[(Token, Position)]) -> Self {
        let mut paths = Self {
            heads: vec![Head::Bare; tokens.len()],
            imports: Vec::new(),
            globs: Vec::new(),
            modules: Vec::new(),
        };
        // The tokens but comments, by their indices.
        let read = (0..tokens.len())
            .filter(|index| !matches!(tokens[*index].0, Token::Comment(_)))
            .collect::<Vec<_>>();
        let token = |at: usize| &tokens[read[at]].0;
        // Within a `use` statement: the path it starts with, once read, and
        // how many groups deep it is.
        let mut statement: Option<(Option<Box<str>>, usize)> = None;
        for at in 0..read.len() {
            let current = token(at);
            if let Some((start, depth)) = &mut statement {
                match current {
                    Token::SemiColon | Token::EOF if *depth == 0 => statement = None,
                    Token::LeftBrace => *depth += 1,
                    Token::RightBrace => *depth = depth.saturating_sub(1),
                    Token::Multiply => paths.globs.push(head(start.as_deref())),
                    _ => {
                        if let Some(name) = word(current) {
                            let start = start.get_or_insert_with(|| name.into());
                            paths.heads[read[at]] = Head::Path(start.clone());
                            if let Token::Identifier(name) = current {
                                paths
                                    .imports
                                    .push((name.as_str().into(), Head::Path(start.clone())));
                            }
                        }
                    }
                }
                continue;
            }
            match current {
                Token::Use => statement = Some((None, 0)),
                Token::Mod => {
                    if let Some(Token::Identifier(name)) = read.get(at + 1).map(|_| token(at + 1)) {
                        paths.modules.push(name.as_str().into());
                    }
                }
                Token::Identifier(_) => paths.heads[read[at]] = Self::head(at, &token),
                _ => {}
            }
        }
        paths
    }

    /// The head of the name at `at` among the read tokens `token` gives:
    /// the first name of the `a::b::` it follows, if any.
    fn head<'t>(at: usize, token: &impl Fn(usize) -> &'t Token) -> Head {
        let follows = |at: usize| at.checked_sub(1).map(token);
        if !matches!(follows(at), Some(Token::DoubleColon)) {
            return Head::Bare;
        }
        // The `::` before the name.
        let mut colons = at - 1;
        loop {
            let Some(start) = colons.checked_sub(1) else {
                return Head::Unknown;
            };
            let Some(name) = word(token(start)) else {
                return Head::Unknown;
            };
            match follows(start) {
                Some(Token::DoubleColon) => colons = start - 1,
                _ => return Head::Path(name.into()),
            }
        }
    }
}

/// The name `token` writes as a path segment: a name, a keyword the lexer
/// keeps as written (`crate`, `super`), or `self`.
fn word(token: &Token) -> Option<&str> {
    match token {
        Token::Identifier(name) | Token::Reserved(name) => Some(name.as_str()),
        Token::SelfValue => Some("self"),
        _ => None,
    }
}

/// The head of a path that starts with `start`: `Unknown` when the text
/// doesn't spell it.
fn head(start: Option<&str>) -> Head {
    start.map_or(Head::Unknown, |start| Head::Path(start.into()))
}

/// Where each name in `tokens` that no visibility marks and a `:` follows
/// starts, after a `{`, a `,` or an attribute's `]`: a private field's
/// among them. A field a macro names (`$name: u32`) isn't, so it is read
/// as public.
fn private_names(tokens: &[(Token, Position)]) -> crate::Shared<[Position]> {
    let mut names = Vec::new();
    let mut before: Option<&Token> = None;
    for (index, (token, position)) in tokens.iter().enumerate() {
        if matches!(token, Token::Comment(_)) {
            continue;
        }
        if matches!(token, Token::Identifier(_))
            && matches!(
                before,
                Some(Token::LeftBrace | Token::Comma | Token::RightBracket)
            )
            && matches!(tokens.get(index + 1), Some((Token::Colon, _)))
        {
            names.push(*position);
        }
        before = Some(token);
    }
    names.into()
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
