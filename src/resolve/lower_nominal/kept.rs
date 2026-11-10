//! The lowerings a runner's compiler keeps from one save to the next
//! (#1646).
//!
//! A source's bodies are lowered again only when something their lowering
//! reads changed: the source's own text, the instances its own bodies'
//! calls take, or what the program holds of the other sources. That is
//! everything the resolved program holds but the other sources' function
//! and method bodies and the instances their calls take: their
//! declarations with their spans, the outputs reveal gave opaque types,
//! the instance tables, constants' bodies, `IndexMut` bodies and the trait
//! objects the sources name, and the program's item numbering. What a
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
        self.pass = interface(program, models).map(|interface| Pass {
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
            digest.update(b"rawr.reveal.source/v1");
            digest.update(interface.program);
            for source in [None, Some(source)] {
                digest.update(interface.calls.get(&source).unwrap_or(&[0; 32]));
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
            .then(|| interface(program, models))
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
        digest.update(b"rawr.lowering.source/v2");
        digest.update(interface.program);
        for source in [None, Some(source)] {
            digest.update(interface.calls.get(&source).unwrap_or(&[0; 32]));
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
    /// The instances each source's bodies' calls take, by the source; those
    /// of the calls `None` holds are every source's.
    calls: BTreeMap<Option<source::Id>, [u8; 32]>,
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
            calls: self.calls,
        }
    }
}

/// What each source's lowering reads of the program; `None` when it can't
/// be written.
fn interface(program: &Program, models: &Models) -> Option<Interface> {
    let mut written = Written(Sha256::new());
    written.0.update(b"rawr.lowering.interface/v2");
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
    // Each source's text, its bodies blank: what a body's lowering reads
    // of another source is never its bodies, but their constants'.
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
    for (index, text) in program.source.texts().enumerate() {
        let id = source::Id::from_index(index);
        framed(&mut written.0, program.source.identity_at(id));
        let spans = bodies.get(&id).map_or(&[][..], Vec::as_slice);
        framed(&mut written.0, &blank(text, spans));
    }
    // The program, its functions' and its impls' methods' bodies' tokens
    // aside: a body's spans and its text when Rawr wrote it stay. A
    // trait's provided bodies stay, as each impl that doesn't override one
    // receives it.
    for shell in program.items.iter() {
        let kind = match &shell.kind {
            item::Kind::Function(function) if !function.body.constant => {
                let mut function = function.clone();
                function.body.syntax = crate::item::BodySyntax::default();
                item::Kind::Function(function)
            }
            item::Kind::Impl(implementation) => {
                let mut implementation = implementation.clone();
                for function in &mut implementation.functions {
                    if !function.body.constant {
                        function.body.syntax = crate::item::BodySyntax::default();
                    }
                }
                item::Kind::Impl(implementation)
            }
            _ => {
                write!(written, "{shell:?}").ok()?;
                continue;
            }
        };
        write!(
            written,
            "{:?}|{:?}|{:?}|{:?}|{:?}|{kind:?}",
            shell.id, shell.source, shell.key, shell.span, shell.facades,
        )
        .ok()?;
    }
    write!(
        written,
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        program.package,
        program.types,
        program.methods,
        program.builtin_methods,
        program.std_traits,
        program.qualified_functions,
        program.qualified_owners,
        program.qualified_pattern_owners,
        program.qualified_rust,
        program.qualified_traits,
        program.trait_names,
        program.entry_crate,
        program.crates,
        program.aliases,
    )
    .ok()?;
    #[cfg(feature = "rust-import")]
    write!(written, "{:?}", program.mirrors).ok()?;
    // Each source's statements outside its items, its items blank: by its
    // words, as a body's length changes how many blanks it is.
    for script in program.scripts.iter() {
        write!(written, "{:?}|{:?}|", script.source, script.namespace).ok()?;
        let words = script.text.split_whitespace().collect::<Vec<_>>();
        framed(&mut written.0, &words.join(" "));
    }
    // The instance tables, each call's instance aside: a body reads only
    // its own calls' (`Interface::calls`).
    let instances = &program.instances;
    write!(
        written,
        "{:?}|{:?}|{:?}|",
        instances.templates, instances.types, instances.impls,
    )
    .ok()?;
    #[cfg(not(target_arch = "wasm32"))]
    write!(written, "{:?}|", instances.natives).ok()?;
    write!(
        written,
        "{:?}|{:?}|{:?}",
        program.lexical, program.macros, models.index_mut,
    )
    .ok()?;
    Some(Interface {
        program: written.0.finalize().into(),
        calls: calls(program)?,
    })
}

/// The instance each call of a template takes, by the source of the body
/// that makes it; a call whose body isn't one of the program's is every
/// source's.
fn calls(program: &Program) -> Option<BTreeMap<Option<source::Id>, [u8; 32]>> {
    let callers = program
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
        .collect::<BTreeMap<_, _>>();
    let mut written = BTreeMap::<Option<source::Id>, Written>::new();
    for ((caller, span), names) in &program.instances.calls {
        let digest = written
            .entry(callers.get(caller).copied())
            .or_insert_with(|| Written(Sha256::new()));
        write!(digest, "{caller:?}|{span:?}|{names:?}|").ok()?;
    }
    Some(
        written
            .into_iter()
            .map(|(source, written)| (source, written.0.finalize().into()))
            .collect(),
    )
}

/// `text` without each of `spans`, its line breaks kept: a body's length
/// changes nothing else.
fn blank(text: &str, spans: &[Span]) -> String {
    let mut output = String::with_capacity(text.len());
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in text.chars() {
        if character == '\n' {
            output.push(character);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        column = column.saturating_add(1);
        let position = Position::new(line, column);
        let inside = spans
            .iter()
            .any(|span| position >= span.start() && position < span.end());
        if !inside {
            output.push(character);
        }
    }
    output
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
