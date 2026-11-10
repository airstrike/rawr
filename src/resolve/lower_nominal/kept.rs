//! The lowerings a runner's compiler keeps from one save to the next
//! (#1646).
//!
//! A source's bodies are lowered again only when something their lowering
//! reads changed: the source's own text, the instances its own bodies'
//! calls take, or what the program holds of the other sources. That is
//! everything the resolved program holds but the other sources' function
//! and method bodies, the instances their calls take and the paths they
//! resolve: their declarations, the outputs reveal gave opaque types, the
//! instance tables, constants' bodies, `IndexMut` bodies and the trait
//! objects the sources name, and the program's item numbering. A
//! declaration's place is read by its line among the lines that hold its
//! source's declarations (`Outline`), so a line a save adds to a body, or
//! a blank or comment line, keeps every other source's lowering. What a
//! source's lowering read outside the program while it compiled (a file
//! `include_str!` reads, a variable `env!` reads) is kept with it, and the
//! lowering is reused only while each reads the same.
//!
//! Reveal's readings of each body for the templates it calls are kept the
//! same way, under what each pass over the program reads of it.

use super::constant::{included, Place};
use super::entry::Lowered;
use super::imported::ImplUse;
use super::models::Models;
use super::op::ExecutableOp;
use super::surface::ImportedUse;
use super::ty::Ty;
use crate::program::source;
use crate::resolve::{item, Program};
use crate::types::position::Lines;
use crate::{host, Engine, Position, Scope, Span};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The part of a source a kept lowering is.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Part {
    /// Its functions' and methods' bodies.
    Bodies,
    /// The root's statements outside its items.
    Script,
}

/// The allocation one `retain_checked` charged.
pub(super) type Charge = (Option<usize>, Option<usize>);

/// What a lowering read outside the program while it compiled: a file
/// `include_str!` or `include_bytes!` read, a variable `env!` read. A kept
/// lowering is reused only while each of its reads reads the same.
#[derive(Clone)]
pub(super) enum Read {
    File {
        path: std::path::PathBuf,
        /// The digest of the bytes it read.
        bytes: [u8; 32],
    },
    Variable {
        place: Place,
        name: Box<str>,
        value: Option<Box<str>>,
    },
}

impl Read {
    /// The read of `bytes` from the file at `path`.
    pub(super) fn file(path: std::path::PathBuf, bytes: &[u8]) -> Self {
        Self::File {
            path,
            bytes: Sha256::digest(bytes).into(),
        }
    }

    /// The read of the variable `name` at `place`, which was `value`.
    pub(super) fn variable(place: Place, name: &str, value: Option<&str>) -> Self {
        Self::Variable {
            place,
            name: name.into(),
            value: value.map(Into::into),
        }
    }

    /// Whether reading again reads the same.
    fn holds(&self, access: &crate::Access) -> bool {
        match self {
            Self::File { path, bytes } => included(access, path)
                .is_ok_and(|read| <[u8; 32]>::from(Sha256::digest(read)) == *bytes),
            Self::Variable { place, name, value } => {
                place.variable(access, name).as_deref() == value.as_deref()
            }
        }
    }
}

impl Models {
    /// Note that the lowering read `read` outside the program: what it
    /// lowers is kept with it.
    pub(super) fn read(&self, read: Read) {
        self.reads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(read);
    }

    /// What the lowering read outside the program since the last take.
    pub(super) fn take_reads(&self) -> Vec<Read> {
        std::mem::take(
            &mut self
                .reads
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// What one part's lowering made, under the key it was made at.
pub(super) struct Kept {
    key: [u8; 32],
    pub(super) lowered: Box<[Lowered]>,
    pub(super) uses: Box<[ImportedUse]>,
    pub(super) dependencies: BTreeSet<host::facade::Dependency>,
    pub(super) outside: Outside,
}

/// What one part's lowering did outside the program as it lowered it.
#[derive(Clone)]
pub(super) struct Outside {
    /// The allocations it charged, in order.
    pub(super) charges: Box<[Charge]>,
    reads: Box<[Read]>,
}

impl Outside {
    /// The allocations a lowering charged, and what it read.
    pub(super) fn new(charges: Vec<Charge>, reads: Vec<Read>) -> Self {
        Self {
            charges: charges.into_boxed_slice(),
            reads: reads.into_boxed_slice(),
        }
    }

    /// Whether each of its reads reads the same again.
    fn holds(&self, access: &crate::Access) -> bool {
        self.reads.iter().all(|read| read.holds(access))
    }

    /// Charge `allocation` again what the lowering charged.
    pub(super) fn charge(
        &self,
        allocation: &mut dyn host::ImportAllocation,
    ) -> Result<(), rawr_import::surface::Error> {
        self.charges
            .iter()
            .try_for_each(|&(items, bytes)| allocation.retain_checked(items, bytes))
    }
}

/// What reveal's reading of one body found (`instance_round`,
/// `body_templates`).
#[derive(Clone, Default)]
pub(super) struct Found {
    /// Each call of a template the body makes: the template, the types
    /// the call gives, and the call's span.
    pub(super) calls: Vec<(crate::item::Key, Vec<Ty>, Span)>,
    pub(super) impl_uses: Vec<ImplUse>,
    /// Whether the body, read with its parameters erased, touches a
    /// crate's types (`reaches_host`).
    pub(super) reaches: bool,
}

/// What reveal's reading of one body found, kept under its key with what
/// the reading did outside the program.
#[derive(Clone)]
struct Scanned {
    found: Found,
    outside: Outside,
}

/// A compiler's kept lowerings (`Engine::resident_lowerings`).
pub(crate) struct Lowerings {
    kept: BTreeMap<(source::Id, Part), Kept>,
    /// Reveal's readings, by their keys.
    scanned: BTreeMap<[u8; 32], Scanned>,
    /// The outlines of the sources the last interface read, by their texts
    /// and bodies (`outlines`).
    outlines: BTreeMap<[u8; 32], Outline>,
    /// How many sources this compiler's compiles lowered rather than
    /// reusing what an earlier compile lowered.
    lowered: usize,
    /// How many sources' bodies this compiler's reveals read rather than
    /// reusing what an earlier reveal read.
    revealed: usize,
}

impl Lowerings {
    /// No lowering kept yet.
    pub(crate) const fn new() -> Self {
        Self {
            kept: BTreeMap::new(),
            scanned: BTreeMap::new(),
            outlines: BTreeMap::new(),
            lowered: 0,
            revealed: 0,
        }
    }

    /// How many sources this compiler's compiles lowered so far.
    pub(crate) fn lowered(&self) -> usize {
        self.lowered
    }

    /// How many sources' bodies this compiler's reveals read so far.
    pub(crate) fn revealed(&self) -> usize {
        self.revealed
    }
}

/// One reveal's use of its compiler's kept readings (`Scanned`). What it
/// keeps goes back to the compiler when it drops: on a failed reveal, with
/// what the earlier reveal kept of the bodies it didn't reach.
pub(super) struct Readings<'e> {
    engine: &'e Engine,
    /// The pass over the program under way; `None` when nothing is kept.
    pass: Option<Pass>,
    earlier: BTreeMap<[u8; 32], Scanned>,
    kept: BTreeMap<[u8; 32], Scanned>,
    read: BTreeSet<source::Id>,
}

/// What a reveal's pass over the program reads of it.
struct Pass {
    interface: Interface,
    /// Each source's digest so far: what its bodies' readings read of it.
    sources: BTreeMap<source::Id, [u8; 32]>,
}

/// What a reveal's pass reads its bodies for.
#[derive(Clone, Copy)]
pub(super) enum Purpose {
    /// The templates each calls (`instance_round`).
    Calls,
    /// Whether it touches a crate's types with its parameters erased
    /// (`body_templates`).
    Erased,
}

impl<'e> Readings<'e> {
    /// The readings of a runner's compiler, which compiles the program
    /// again on each save; `None` for any other compiler.
    pub(super) fn open(engine: &'e Engine) -> Option<Self> {
        engine.imported_native.as_ref()?;
        let earlier = std::mem::take(&mut lock(engine).scanned);
        Some(Self {
            engine,
            pass: None,
            earlier,
            kept: BTreeMap::new(),
            read: BTreeSet::new(),
        })
    }

    /// Begin a pass over `program` as it is now, whose bodies' readings
    /// read `models`.
    pub(super) fn begin(&mut self, program: &Program, models: &Models) {
        self.pass = interface(self.engine, program, models).map(|interface| Pass {
            interface,
            sources: BTreeMap::new(),
        });
    }

    /// What reading `body` finds (`read`): what an earlier reading under
    /// the same key found, when each of its reads outside the program
    /// reads the same, charged again to `allocation`.
    pub(super) fn read(
        &mut self,
        body: Reading<'_>,
        models: &Models,
        allocation: &mut dyn host::ImportAllocation,
        read: impl FnOnce(&mut dyn host::ImportAllocation) -> Result<Found, rawr_import::surface::Error>,
    ) -> Result<Found, rawr_import::surface::Error> {
        let Some(pass) = &mut self.pass else {
            return read(allocation);
        };
        let key = pass.key(body);
        if let Some(earlier) = self.earlier.remove(&key) {
            self.kept.entry(key).or_insert(earlier);
        }
        if let Some(scanned) = self
            .kept
            .get(&key)
            .filter(|scanned| scanned.outside.holds(&models.access))
        {
            scanned.outside.charge(allocation)?;
            return Ok(scanned.found.clone());
        }
        models.take_reads();
        let mut recording = Recording {
            inner: allocation,
            charges: Vec::new(),
        };
        let found = read(&mut recording)?;
        self.read.insert(body.source);
        self.kept.insert(
            key,
            Scanned {
                found: found.clone(),
                outside: Outside::new(recording.charges, models.take_reads()),
            },
        );
        Ok(found)
    }

    /// The reveal succeeded: what the earlier one kept and this one didn't
    /// reuse is gone.
    pub(super) fn succeeded(&mut self) {
        self.earlier.clear();
    }
}

impl Drop for Readings<'_> {
    fn drop(&mut self) {
        let mut kept = std::mem::take(&mut self.earlier);
        kept.append(&mut self.kept);
        let mut lowerings = lock(self.engine);
        lowerings.scanned = kept;
        lowerings.revealed += self.read.len();
    }
}

/// A body a reveal's pass reads (`Readings::read`).
#[derive(Clone, Copy)]
pub(super) struct Reading<'a> {
    pub(super) program: &'a Program,
    /// The source the body is in.
    pub(super) source: source::Id,
    /// The function's or the method's key.
    pub(super) key: &'a crate::item::Key,
    pub(super) purpose: Purpose,
}

impl Pass {
    /// The key of `body`'s reading: what it reads of the program, and of
    /// its source.
    fn key(&mut self, body: Reading<'_>) -> [u8; 32] {
        let Reading {
            program,
            source,
            key,
            purpose,
        } = body;
        let interface = &self.interface;
        let of_source = *self.sources.entry(source).or_insert_with(|| {
            let mut digest = Sha256::new();
            digest.update(b"rawr.reveal.source/v2");
            digest.update(interface.program);
            for source in [None, Some(source)] {
                digest.update(interface.own.get(&source).unwrap_or(&[0; 32]));
            }
            digest.update(source.index().to_le_bytes());
            framed(&mut digest, program.source.text_at(source));
            digest.finalize().into()
        });
        let mut written = Written(Sha256::new());
        written.0.update(of_source);
        written.0.update([match purpose {
            Purpose::Calls => 0,
            Purpose::Erased => 1,
        }]);
        // A key's `Debug` can't fail to write.
        let _ = write!(written, "{key:?}");
        written.0.finalize().into()
    }
}

/// One compile's use of its compiler's kept lowerings. What it keeps goes
/// back to the compiler when it drops: on a failed compile, with what the
/// earlier compile kept of the sources it didn't reach.
pub(super) struct Session<'e> {
    engine: &'e Engine,
    /// What each source's lowering reads of the program; `None` when
    /// nothing is kept.
    interface: Option<Interface>,
    earlier: BTreeMap<(source::Id, Part), Kept>,
    kept: BTreeMap<(source::Id, Part), Kept>,
    lowered: BTreeSet<source::Id>,
}

impl<'e> Session<'e> {
    /// The session of a runner's compiler, which compiles the program
    /// again on each save; `None` for any other compiler.
    pub(super) fn open(
        engine: &'e Engine,
        program: &Program,
        models: &Models,
        scope: Option<&Scope>,
        optimization_level: crate::OptimizationLevel,
    ) -> Option<Self> {
        engine.imported_native.as_ref()?;
        let earlier = std::mem::take(&mut lock(engine).kept);
        let interface = scope
            .is_none_or(Scope::is_empty)
            .then(|| interface(engine, program, models))
            .flatten()
            .map(|interface| interface.compiled_at(optimization_level));
        Some(Self {
            engine,
            interface,
            earlier,
            kept: BTreeMap::new(),
            lowered: BTreeSet::new(),
        })
    }

    /// The key of `part` of `source`, whose bodies' texts are `texts`:
    /// `None` when nothing is kept.
    pub(super) fn key(
        &self,
        part: Part,
        source: source::Id,
        program: &Program,
        texts: &[&str],
    ) -> Option<[u8; 32]> {
        let interface = self.interface.as_ref()?;
        let mut digest = Sha256::new();
        digest.update(b"rawr.lowering.source/v3");
        digest.update(interface.program);
        for source in [None, Some(source)] {
            digest.update(interface.own.get(&source).unwrap_or(&[0; 32]));
        }
        digest.update([u8::from(part == Part::Script)]);
        digest.update(source.index().to_le_bytes());
        framed(&mut digest, program.source.text_at(source));
        for text in texts {
            framed(&mut digest, text);
        }
        Some(digest.finalize().into())
    }

    /// What the earlier compile kept of `part` of `source` under `key`,
    /// when each of its reads outside the program reads the same.
    pub(super) fn reuse(
        &mut self,
        part: Part,
        source: source::Id,
        key: [u8; 32],
        access: &crate::Access,
    ) -> Option<&Kept> {
        let kept = self.earlier.remove(&(source, part)).filter(|kept| {
            kept.key == key && kept.outside.reads.iter().all(|read| read.holds(access))
        })?;
        Some(self.kept.entry((source, part)).or_insert(kept))
    }

    /// Keep what `part` of `source` lowered to under `key`, when there is
    /// one.
    pub(super) fn keep(
        &mut self,
        part: Part,
        source: source::Id,
        key: Option<[u8; 32]>,
        lowered: &[Lowered],
        uses: &[ImportedUse],
        dependencies: &BTreeSet<host::facade::Dependency>,
        outside: Outside,
    ) {
        self.lowered.insert(source);
        let Some(key) = key else {
            return;
        };
        self.kept.insert(
            (source, part),
            Kept {
                key,
                lowered: lowered.to_vec().into_boxed_slice(),
                uses: uses.to_vec().into_boxed_slice(),
                dependencies: dependencies.clone(),
                outside,
            },
        );
    }

    /// The compile succeeded: what the earlier one kept and this one
    /// didn't reuse is gone.
    pub(super) fn succeeded(&mut self) {
        self.earlier.clear();
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        let mut kept = std::mem::take(&mut self.earlier);
        kept.append(&mut self.kept);
        let mut lowerings = lock(self.engine);
        lowerings.kept = kept;
        lowerings.lowered += self.lowered.len();
    }
}

impl Lowered {
    /// This kept lowering for a new generation of the program: what an op
    /// holds of the generation it was made in made again. `None` when one
    /// can't be, and the body is lowered again.
    pub(super) fn renewed(&self, models: &Models) -> Option<Self> {
        let mut lowered = self.clone();
        lowered.ops = self
            .ops
            .iter()
            .map(|op| op.renewed(models))
            .collect::<Option<_>>()?;
        Some(lowered)
    }
}

/// An allocation that remembers what it charged, which a kept lowering
/// charges again when it is reused.
pub(super) struct Recording<'a> {
    pub(super) inner: &'a mut dyn host::ImportAllocation,
    pub(super) charges: Vec<Charge>,
}

impl host::ImportAllocation for Recording<'_> {
    fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), rawr_import::surface::Error> {
        self.inner.retain_checked(items, bytes)?;
        self.charges.push((items, bytes));
        Ok(())
    }

    fn support_owner(&self) -> Option<crate::package::import::runner::account::support::Owner> {
        self.inner.support_owner()
    }
}

fn lock(engine: &Engine) -> std::sync::MutexGuard<'_, Lowerings> {
    engine
        .resident_lowerings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `text`, after its length, so two texts never run together.
fn framed(digest: &mut Sha256, text: &str) {
    digest.update(text.len().to_le_bytes());
    digest.update(text.as_bytes());
}

/// A digest written to as text, as `Debug` writes a value.
struct Written(Sha256);

impl std::fmt::Write for Written {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.update(text.as_bytes());
        Ok(())
    }
}

/// What each source's lowering reads of the program, the other sources'
/// bodies aside.
struct Interface {
    /// What every source's lowering reads.
    program: [u8; 32],
    /// What only one source's bodies read, by the source (`own`); what
    /// `None` holds is every source's.
    own: BTreeMap<Option<source::Id>, [u8; 32]>,
}

impl Interface {
    /// What a compile at `level` reads, which optimizes the text it lowers.
    fn compiled_at(self, level: crate::OptimizationLevel) -> Self {
        let mut written = Written(Sha256::new());
        written.0.update(self.program);
        // A level's `Debug` can't fail to write.
        let _ = write!(written, "{level:?}");
        Self {
            program: written.0.finalize().into(),
            own: self.own,
        }
    }
}

/// What each source's lowering reads of the program; `None` when it can't
/// be written.
///
/// Every position of a source is written at its line's number among the
/// lines that hold the source's tokens outside its bodies (`Outline`): a
/// save that adds or removes a line in a body, or a blank or comment line,
/// moves the source's later declarations without changing what another
/// source's lowering reads of them, which is never their place.
fn interface(engine: &Engine, program: &Program, models: &Models) -> Option<Interface> {
    let mut written = Written(Sha256::new());
    written.0.update(b"rawr.lowering.interface/v3");
    // The compiler: the facades its host registry holds, and its options.
    write!(
        written,
        "{}|{:?}|{:?}|{:?}",
        Engine::registered_facades(),
        models.access,
        models.features,
        models.std_traits,
    )
    .ok()?;
    // Each source's tokens, its bodies' aside: what a body's lowering
    // reads of another source is never its bodies, but their constants'.
    let outlines = outlines(engine, program);
    for (index, outline) in outlines.sources.iter().enumerate() {
        framed(
            &mut written.0,
            program.source.identity_at(source::Id::from_index(index)),
        );
        written.0.update(outline.tokens);
    }
    let lines = |source: source::Id| {
        outlines
            .sources
            .get(source.index())
            .map(|outline| outline.lines.clone())
            .unwrap_or_default()
    };
    let sources = program
        .items
        .iter()
        .map(|shell| (shell.id, shell.source))
        .collect::<BTreeMap<_, _>>();
    // The program, its functions' and its impls' methods' bodies' tokens
    // aside: a body's spans and its text when Rawr wrote it stay. A
    // trait's provided bodies stay, as each impl that doesn't override one
    // receives it.
    for shell in program.items.iter() {
        let cleared;
        let kind = match &shell.kind {
            item::Kind::Function(function) if !function.body.constant => {
                let mut function = function.clone();
                function.body.syntax = crate::item::BodySyntax::default();
                cleared = item::Kind::Function(function);
                &cleared
            }
            item::Kind::Impl(implementation) => {
                let mut implementation = implementation.clone();
                for function in &mut implementation.functions {
                    if !function.body.constant {
                        function.body.syntax = crate::item::BodySyntax::default();
                    }
                }
                cleared = item::Kind::Impl(implementation);
                &cleared
            }
            kind => kind,
        };
        lines(shell.source)
            .writing(|| {
                write!(
                    written,
                    "{:?}|{:?}|{:?}|{:?}|{:?}|{kind:?}",
                    shell.id, shell.source, shell.key, shell.span, shell.facades,
                )
            })
            .ok()?;
    }
    for definition in program.types.iter() {
        let lines = sources
            .get(&definition.item())
            .map(|source| lines(*source))
            .unwrap_or_default();
        lines.writing(|| write!(written, "{definition:?}|")).ok()?;
    }
    for definition in program.methods.iter().chain(&program.builtin_methods) {
        lines(definition.source())
            .writing(|| write!(written, "{definition:?}|"))
            .ok()?;
    }
    for ((source, name), alias) in &program.aliases {
        lines(*source)
            .writing(|| write!(written, "{source:?}|{name:?}|{alias:?}|"))
            .ok()?;
    }
    write!(
        written,
        "{:?}|{:?}|{:?}|{:?}|{:?}",
        program.package,
        program.std_traits,
        program.trait_names,
        program.entry_crate,
        program.crates,
    )
    .ok()?;
    #[cfg(feature = "rust-import")]
    write!(written, "{:?}", program.mirrors).ok()?;
    // Each source's statements outside its items, its items blank: by its
    // tokens, as a body's length changes how many blanks it is, and a
    // comment is none.
    for (script, outline) in program.scripts.iter().zip(&outlines.scripts) {
        write!(written, "{:?}|{:?}|", script.source, script.namespace).ok()?;
        written.0.update(outline.tokens);
    }
    // The instance tables, each call's instance aside: a body reads only
    // its own calls' (`Interface::own`).
    let callers = callers(program);
    let instances = &program.instances;
    write!(written, "{:?}|", instances.templates).ok()?;
    for (key, types) in &instances.types {
        let lines = callers
            .get(key)
            .map(|source| lines(*source))
            .unwrap_or_default();
        lines
            .writing(|| write!(written, "{key:?}|{types:?}|"))
            .ok()?;
    }
    for (index, instances) in &instances.impls {
        let lines = program
            .items
            .get(*index)
            .map(|shell| lines(shell.source))
            .unwrap_or_default();
        lines
            .writing(|| write!(written, "{index}|{instances:?}|"))
            .ok()?;
    }
    #[cfg(not(target_arch = "wasm32"))]
    write!(written, "{:?}|", instances.natives).ok()?;
    // The program's macros and `IndexMut` bodies, which a body's lowering
    // reads with their places: at those places.
    write!(written, "{:?}|{:?}", program.macros, models.index_mut).ok()?;
    Some(Interface {
        program: written.0.finalize().into(),
        own: own(program, &callers)?,
    })
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
fn own(
    program: &Program,
    callers: &BTreeMap<&crate::item::Key, source::Id>,
) -> Option<BTreeMap<Option<source::Id>, [u8; 32]>> {
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

/// What another source's lowering reads of one source's text (#1646): its
/// tokens outside its function and method bodies, and its lines, each
/// numbered among the lines that hold one of those tokens.
#[derive(Clone)]
struct Outline {
    tokens: [u8; 32],
    lines: Lines,
}

impl Outline {
    /// The outline of `text`, whose function and method bodies are
    /// `bodies`, in order.
    fn new(engine: &Engine, text: &str, bodies: &[Span]) -> Self {
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
        let mut held = vec![false; text.matches('\n').count() + 1];
        let mut written = Written(Sha256::new());
        let mut next = 0;
        for (token, position) in super::lex_rust(engine, text) {
            if token == crate::tokenizer::Token::EOF {
                break;
            }
            while merged.get(next).is_some_and(|(_, end)| *end <= position) {
                next += 1;
            }
            if merged
                .get(next)
                .is_some_and(|(start, _)| *start <= position)
            {
                continue;
            }
            // A token's `Debug` can't fail to write.
            let _ = write!(written, "{token:?}|");
            if let Some(line) = position.line().and_then(|line| held.get_mut(line - 1)) {
                *line = true;
            }
        }
        let mut number = 0_u16;
        let numbers = held
            .into_iter()
            .map(|held| {
                number = number.saturating_add(u16::from(held));
                number
            })
            .collect();
        Self {
            tokens: written.0.finalize().into(),
            lines: Lines::new(numbers),
        }
    }
}

/// The outlines of the program's sources and scripts. A compiler keeps the
/// outlines it wrote, and writes again only a text or bodies that changed.
fn outlines(engine: &Engine, program: &Program) -> Outlines {
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
    let mut earlier = std::mem::take(&mut lock(engine).outlines);
    let mut kept = BTreeMap::new();
    let mut outline = |text: &str, spans: &[Span]| {
        let mut written = Written(Sha256::new());
        framed(&mut written.0, text);
        // A span's `Debug` can't fail to write.
        let _ = write!(written, "{spans:?}");
        let key: [u8; 32] = written.0.finalize().into();
        let outline = earlier
            .remove(&key)
            .or_else(|| kept.get(&key).cloned())
            .unwrap_or_else(|| Outline::new(engine, text, spans));
        kept.insert(key, outline.clone());
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
    lock(engine).outlines = kept;
    Outlines { sources, scripts }
}

/// The outlines of a program's sources and of their scripts, each by its
/// index.
struct Outlines {
    sources: Vec<Outline>,
    scripts: Vec<Outline>,
}

impl ExecutableOp {
    /// This op for a new generation of the program: a default value made
    /// again in its model, a witnessed op with no instance yet. `None` when
    /// the default can't be made.
    fn renewed(&self, models: &Models) -> Option<Self> {
        Some(match self {
            Self::Default { definition, .. } => Self::Default {
                value: models
                    .runtime
                    .materialize_default(
                        &crate::type_def::Identity::Script(definition.key().clone()),
                        definition.schema(),
                    )
                    .ok()?,
                definition: definition.clone(),
            },
            Self::Witnessed(op) => Self::Witnessed(Box::new(super::op::WitnessedOp::new(
                op.template.renewed(models)?,
                op.witnesses,
            ))),
            op => op.clone(),
        })
    }
}
