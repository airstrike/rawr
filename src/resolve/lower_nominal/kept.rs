//! The lowerings a runner's compiler keeps from one save to the next
//! (#1646).
//!
//! A source's bodies are lowered again only when something their lowering
//! reads changed: the source's own text, the instances its own bodies'
//! calls take, or what it reads of the program (`Interface`): what every
//! source reads, and the other sources' declarations its text reaches by
//! name, their bodies and inert attributes aside (#1718). A line, a
//! comment, a lint or a declaration a save adds, or a declaration written
//! over more lines, keeps the lowering of each source that doesn't name
//! what changed. What a source's lowering read outside the program while
//! it compiled (a file `include_str!` reads, a variable `env!` reads) is
//! kept with it, and the lowering is reused only while each reads the
//! same.
//!
//! A kept lowering also holds each adapter its ops call, a witnessed op's
//! among them, and is reused only while the runner admitted each one: a
//! runner whose files another runner wrote may lack one, and the part is
//! lowered again, which prepares it (#1734).
//!
//! Reveal's readings of each body for the templates it calls are kept the
//! same way, under what each pass over the program reads of it.
//!
//! An owner's check of the program, which checks it again on each save,
//! keeps the same way what each source's check found: the crates' items
//! its bodies use and the facades they depend on (`Checks`). Its key is
//! what the source's lowering reads, and the applications the check's
//! registry admits, so a save that changes one source's private field
//! lowers again only that source and those that name the type.

use super::constant::{included, Place};
use super::disk::{self, Disk};
use super::entry::{Body, Lowered};
use super::imported::ImplUse;
use super::interface::{framed, Digests, Interface, Native, Outlined, Written};
use super::models::Models;
use super::op::ExecutableOp;
use super::splice::Parsed;
use super::surface::ImportedUse;
use super::ty::Ty;
use crate::program::source;
use crate::resolve::package::Resolutions;
use crate::resolve::Program;
use crate::{host, Engine, Scope, Span};
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
#[derive(Clone, serde::Deserialize, serde::Serialize)]
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
#[derive(serde::Deserialize, serde::Serialize)]
pub(super) struct Kept {
    key: [u8; 32],
    pub(super) lowered: Box<[Lowered]>,
    pub(super) uses: Box<[ImportedUse]>,
    pub(super) dependencies: BTreeSet<host::facade::Dependency>,
    /// Each adapter its ops call, a witnessed op's among them (#1734).
    adapters: Box<[host::structural::Definition]>,
    pub(super) outside: Outside,
    /// What the compile parsed of the part's text (#1877). A runner's
    /// file holds none: its first compile parses the text again.
    #[serde(skip)]
    parsed: Option<Parsed>,
}

impl Kept {
    /// Whether `registry`'s runner admitted each adapter the kept ops call.
    fn admitted(&self, registry: &host::Registry) -> bool {
        self.adapters
            .iter()
            .all(|adapter| registry.admits_structural(adapter))
    }
}

/// What one part's lowering did outside the program as it lowered it.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
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
#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
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
#[derive(Clone, serde::Deserialize, serde::Serialize)]
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
    outlines: Outlined,
    /// The digests of the items the last interface read (#1878).
    digests: Digests,
    /// What the last interface read of the native surface, by how many
    /// facades were registered when it was read: a runner's image doesn't
    /// change while it runs (#1725).
    native: Option<(usize, crate::Shared<Native>)>,
    /// How many sources this compiler's compiles lowered rather than
    /// reusing what an earlier compile lowered.
    lowered: usize,
    /// How many sources' bodies this compiler's reveals read rather than
    /// reusing what an earlier reveal read.
    revealed: usize,
    /// What each source's check of the program found (`Checks`).
    checks: BTreeMap<source::Id, Checked>,
    /// How many sources this compiler's checks lowered rather than
    /// reusing what an earlier check found.
    checked: usize,
    /// Where a runner keeps them for the next runner (#1638).
    disk: Option<Disk>,
    /// What this compiler's compiles resolved of each unit (#1876).
    resolutions: Resolutions,
}

impl Lowerings {
    /// No lowering kept yet.
    pub(crate) const fn new() -> Self {
        Self {
            kept: BTreeMap::new(),
            scanned: BTreeMap::new(),
            outlines: BTreeMap::new(),
            digests: Digests::new(),
            native: None,
            lowered: 0,
            revealed: 0,
            checks: BTreeMap::new(),
            checked: 0,
            disk: None,
            resolutions: Resolutions::new(),
        }
    }

    /// Keep each lowering and reading in `disk` as well, where this
    /// compiler's first compile finds what an earlier runner kept.
    pub(crate) fn store(&mut self, disk: Disk) {
        self.disk = Some(disk);
    }

    /// How many sources this compiler's compiles lowered so far.
    pub(crate) fn lowered(&self) -> usize {
        self.lowered
    }

    /// How many sources' bodies this compiler's reveals read so far.
    pub(crate) fn revealed(&self) -> usize {
        self.revealed
    }

    /// How many sources this compiler's checks lowered so far.
    pub(crate) fn checked(&self) -> usize {
        self.checked
    }
}

/// Whether `engine` keeps its lowerings from one compile to the next: a
/// runner's compiler, or an owner's check of the program (#1646).
fn keeps(engine: &Engine) -> bool {
    engine.imported_native.is_some() || engine.keeps_lowerings
}

/// `graph` resolved, misses against native code recorded in `native`: a
/// compiler that keeps its lowerings reuses each unit's resolution its
/// last compile kept while what it reads is as it was (#1876).
pub(crate) fn resolve(
    engine: &Engine,
    graph: crate::package::Graph,
    native: std::sync::Arc<crate::resolve::native::Capture>,
) -> Result<Program, crate::resolve::package::Failure> {
    if !keeps(engine) {
        return crate::resolve::package::resolve_captured(graph, &engine.host_registry, native);
    }
    let mut resolutions = std::mem::take(&mut lock(engine).resolutions);
    let resolved = crate::resolve::package::resolve_kept(
        graph,
        &engine.host_registry,
        Some(native),
        &mut resolutions,
    );
    lock(engine).resolutions = resolutions;
    resolved
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
    /// Whether this is the compiler's first reveal, which reads what an
    /// earlier runner kept on disk (#1638).
    first: bool,
    /// The sources whose readings it read from disk.
    loaded: BTreeSet<source::Id>,
    /// The keys of each source's readings this reveal.
    keys: BTreeMap<source::Id, BTreeSet<[u8; 32]>>,
}

/// A source's readings on disk hold each one's own key (`Readings::read`).
const READINGS: [u8; 32] = [0; 32];

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
    /// The readings of a compiler that compiles or checks the program
    /// again on each save (`keeps`); `None` for any other compiler.
    pub(super) fn open(engine: &'e Engine) -> Option<Self> {
        if !keeps(engine) {
            return None;
        }
        let earlier = std::mem::take(&mut lock(engine).scanned);
        Some(Self {
            engine,
            pass: None,
            first: earlier.is_empty(),
            earlier,
            kept: BTreeMap::new(),
            read: BTreeSet::new(),
            loaded: BTreeSet::new(),
            keys: BTreeMap::new(),
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
    /// reads the same, charged again to `allocation`. A compiler's first
    /// reveal finds the earlier readings of a source in its file.
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
        if self.first && self.loaded.insert(body.source) {
            let entry = disk::Entry {
                identity: body.program.source.identity_at(body.source),
                kind: disk::Kind::Readings,
                key: &READINGS,
            };
            let stored = lock(self.engine)
                .disk
                .as_mut()
                .and_then(|disk| disk.read::<Vec<([u8; 32], Scanned)>>(self.engine, entry));
            self.earlier.extend(stored.into_iter().flatten());
        }
        self.keys.entry(body.source).or_default().insert(key);
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

    /// The reveal of `program` succeeded: what the earlier one kept and
    /// this one didn't reuse is gone. Each source it read a body of again
    /// keeps its readings on disk.
    pub(super) fn succeeded(&mut self, program: &Program) {
        self.earlier.clear();
        let mut lowerings = lock(self.engine);
        let Some(disk) = lowerings.disk.as_mut() else {
            return;
        };
        for source in &self.read {
            let readings = self
                .keys
                .get(source)
                .into_iter()
                .flatten()
                .filter_map(|key| self.kept.get(key).map(|scanned| (key, scanned)));
            let entry = disk::Entry {
                identity: program.source.identity_at(*source),
                kind: disk::Kind::Readings,
                key: &READINGS,
            };
            disk.write(self.engine, entry, &readings.collect::<Vec<_>>());
        }
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
            digest.update(b"rawr.reveal.source/v3");
            digest.update(interface.of(source));
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
    /// The level the compile optimizes the text it lowers at.
    level: crate::OptimizationLevel,
    earlier: BTreeMap<(source::Id, Part), Kept>,
    kept: BTreeMap<(source::Id, Part), Kept>,
    lowered: BTreeSet<source::Id>,
    /// Whether this is the compiler's first compile, which reads what an
    /// earlier runner kept on disk (#1638).
    first: bool,
}

/// The key `part` of `source` is kept under (`Session::key`).
#[derive(Clone, Copy)]
pub(super) struct Key<'a> {
    part: Part,
    source: source::Id,
    /// The source's path, which names its file on disk.
    identity: &'a str,
    digest: [u8; 32],
}

impl Key<'_> {
    fn entry(&self) -> disk::Entry<'_> {
        disk::Entry {
            identity: self.identity,
            kind: disk::Kind::Part(self.part),
            key: &self.digest,
        }
    }
}

impl<'e> Session<'e> {
    /// The session of a compiler that compiles or checks the program
    /// again on each save (`keeps`); `None` for any other compiler.
    pub(super) fn open(
        engine: &'e Engine,
        program: &Program,
        models: &Models,
        scope: Option<&Scope>,
        optimization_level: crate::OptimizationLevel,
    ) -> Option<Self> {
        if !keeps(engine) {
            return None;
        }
        let earlier = std::mem::take(&mut lock(engine).kept);
        let interface = scope
            .is_none_or(Scope::is_empty)
            .then(|| interface(engine, program, models))
            .flatten();
        Some(Self {
            engine,
            interface,
            level: optimization_level,
            first: earlier.is_empty(),
            earlier,
            kept: BTreeMap::new(),
            lowered: BTreeSet::new(),
        })
    }

    /// The key of `part` of `source`, whose bodies are `bodies`: `None`
    /// when nothing is kept. Their texts are the source's, by where each
    /// body is, so none is built for the key (#1850).
    pub(super) fn key<'p>(
        &self,
        part: Part,
        source: source::Id,
        program: &'p Program,
        bodies: &[Body<'_>],
    ) -> Option<Key<'p>> {
        let interface = self.interface.as_ref()?;
        let mut written = Written(Sha256::new());
        written.0.update(b"rawr.lowering.source/v5");
        written.0.update(interface.of(source));
        // A level's `Debug` can't fail to write.
        let _ = write!(written, "{:?}", self.level);
        written.0.update([u8::from(part == Part::Script)]);
        written.0.update(source.index().to_le_bytes());
        framed(&mut written.0, program.source.text_at(source));
        for body in bodies {
            body.write_key(&mut written);
        }
        Some(Key {
            part,
            source,
            identity: program.source.identity_at(source),
            digest: written.0.finalize().into(),
        })
    }

    /// What the earlier compile kept of its part under `key`, when each
    /// of its reads outside the program reads the same and the runner
    /// admitted each adapter it calls (#1734). A compiler's first compile
    /// finds it in the part's file.
    pub(super) fn reuse(&mut self, key: Key<'_>, models: &Models) -> Option<&Kept> {
        let place = (key.source, key.part);
        let earlier = match self.earlier.remove(&place) {
            Some(kept) => Some(kept),
            None if self.first => lock(self.engine)
                .disk
                .as_mut()
                .and_then(|disk| disk.read::<Kept>(self.engine, key.entry())),
            None => None,
        };
        let kept = earlier.filter(|kept| {
            kept.key == key.digest
                && kept.outside.holds(&models.access)
                && kept.admitted(&models.host)
        })?;
        Some(self.kept.entry(place).or_insert(kept))
    }

    /// Keep what a part of `source` lowered to under `key`, when there is
    /// one, in memory and in its file.
    pub(super) fn keep(
        &mut self,
        source: source::Id,
        key: Option<Key<'_>>,
        lowered: &[Lowered],
        uses: &[ImportedUse],
        dependencies: &BTreeSet<host::facade::Dependency>,
        outside: Outside,
    ) {
        self.lowered.insert(source);
        let Some(key) = key else {
            return;
        };
        let kept = Kept {
            key: key.digest,
            lowered: lowered.to_vec().into_boxed_slice(),
            uses: uses.to_vec().into_boxed_slice(),
            dependencies: dependencies.clone(),
            adapters: adapters(lowered),
            outside,
            parsed: None,
        };
        if let Some(disk) = lock(self.engine).disk.as_mut() {
            disk.write(self.engine, key.entry(), &kept);
        }
        self.kept.insert((source, key.part), kept);
    }

    /// What a compile parsed of the text of `part` of `source`, which this
    /// compile reused or kept (#1877).
    pub(super) fn parsed(&self, source: source::Id, part: Part) -> Option<&Parsed> {
        self.kept.get(&(source, part))?.parsed.as_ref()
    }

    /// Keep `parsed`, this compile's parse of the text of `part` of
    /// `source`, with the part, when this compile reused or kept it.
    pub(super) fn keep_parsed(&mut self, source: source::Id, part: Part, parsed: Parsed) {
        if let Some(kept) = self.kept.get_mut(&(source, part)) {
            kept.parsed = Some(parsed);
        }
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

/// What one source's check found (`imported`): the crates' items its
/// bodies use, the facades they depend on, and what the check did outside
/// the program, under the key it was checked at.
pub(super) struct Checked {
    key: [u8; 32],
    pub(super) uses: Box<[ImportedUse]>,
    pub(super) dependencies: BTreeSet<host::facade::Dependency>,
    pub(super) outside: Outside,
}

/// One check's use of its compiler's kept checks: a source is lowered
/// again only when something its check reads changed, as a runner's
/// compile lowers a source again (`Session`). What it keeps goes back to
/// the compiler when it drops: on a failed check, with what the earlier
/// check kept of the sources it didn't reach.
pub(super) struct Checks<'e> {
    engine: &'e Engine,
    /// What each source's check reads of the program.
    interface: Interface,
    /// What every source's check reads besides: the applications the
    /// registry admits, which answer calls nothing else does (D650).
    applied: [u8; 32],
    earlier: BTreeMap<source::Id, Checked>,
    kept: BTreeMap<source::Id, Checked>,
}

impl<'e> Checks<'e> {
    /// The checks of a compiler that keeps its lowerings; `None` for any
    /// other compiler, or when what the sources read can't be written.
    pub(super) fn open(engine: &'e Engine, program: &Program, models: &Models) -> Option<Self> {
        if !keeps(engine) {
            return None;
        }
        let interface = interface(engine, program, models)?;
        let mut applied = Written(Sha256::new());
        applied.0.update(b"rawr.check.applied/v1");
        for (key, item) in models.host.applications() {
            write!(applied, "{key:?}|{item:?}|").ok()?;
        }
        let earlier = std::mem::take(&mut lock(engine).checks);
        Some(Self {
            engine,
            interface,
            applied: applied.0.finalize().into(),
            earlier,
            kept: BTreeMap::new(),
        })
    }

    /// The key `source` is checked at, whose bodies are `bodies`, in the
    /// order the check lowers them: each its item's key (`None` for the
    /// source's statements outside its items) and the body, whose text is
    /// the source's by where it is (`Session::key`).
    pub(super) fn key<'b>(
        &self,
        source: source::Id,
        program: &Program,
        bodies: impl Iterator<Item = (Option<&'b crate::item::Key>, &'b Body<'b>)>,
    ) -> [u8; 32] {
        let mut written = Written(Sha256::new());
        written.0.update(b"rawr.check.source/v2");
        written.0.update(self.interface.of(source));
        written.0.update(self.applied);
        written.0.update(source.index().to_le_bytes());
        framed(&mut written.0, program.source.text_at(source));
        for (key, body) in bodies {
            // A key's `Debug` can't fail to write.
            let _ = write!(written, "{key:?}");
            body.write_key(&mut written);
        }
        written.0.finalize().into()
    }

    /// What the earlier check found of `source` under `key`, when each of
    /// its reads outside the program reads the same.
    pub(super) fn reuse(
        &mut self,
        source: source::Id,
        key: [u8; 32],
        models: &Models,
    ) -> Option<&Checked> {
        let earlier = self
            .earlier
            .remove(&source)
            .filter(|earlier| earlier.key == key && earlier.outside.holds(&models.access))?;
        Some(self.kept.entry(source).or_insert(earlier))
    }

    /// Keep what the check of `source` found under `key`.
    pub(super) fn keep(
        &mut self,
        source: source::Id,
        key: [u8; 32],
        uses: Vec<ImportedUse>,
        dependencies: BTreeSet<host::facade::Dependency>,
        outside: Outside,
    ) {
        self.kept.insert(
            source,
            Checked {
                key,
                uses: uses.into_boxed_slice(),
                dependencies,
                outside,
            },
        );
    }

    /// The check succeeded: what the earlier one kept and this one didn't
    /// reuse is gone.
    pub(super) fn succeeded(&mut self) {
        self.earlier.clear();
    }
}

impl Drop for Checks<'_> {
    fn drop(&mut self) {
        let mut kept = std::mem::take(&mut self.earlier);
        kept.append(&mut self.kept);
        lock(self.engine).checks = kept;
    }
}

/// Count `sources` a check of the program lowered, rather than reusing an
/// earlier check's lowering, toward `engine`'s.
pub(super) fn checked(engine: &Engine, sources: usize) {
    lock(engine).checked += sources;
}

/// Each adapter the ops of `lowered` call, once.
fn adapters(lowered: &[Lowered]) -> Box<[host::structural::Definition]> {
    let mut adapters = BTreeSet::new();
    for op in lowered.iter().flat_map(|lowered| &lowered.ops) {
        op.adapters(&mut adapters);
    }
    adapters.into_iter().cloned().collect()
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

/// What each source's lowering reads of `program`; `None` when it can't
/// be written. The compiler keeps the outlines of its sources' texts, and
/// what they read of its native surface.
fn interface(engine: &Engine, program: &Program, models: &Models) -> Option<Interface> {
    let registered = Engine::registered_facades();
    let (mut outlined, mut digests, kept) = {
        let mut lowerings = lock(engine);
        let kept = lowerings
            .native
            .take()
            .filter(|(at, native)| *at == registered && native.is_of(&program.own));
        (
            std::mem::take(&mut lowerings.outlines),
            std::mem::replace(&mut lowerings.digests, Digests::new()),
            kept,
        )
    };
    let native = kept.map_or_else(
        || crate::Shared::new(Native::new(program, &models.host)),
        |(_, native)| native,
    );
    // A facade's proof is what every source reads of another crate's
    // facade, and of the program's own, its entries a name reaches.
    let interface = crate::host::facade::Fingerprint::rooted(|| {
        Interface::new(
            engine,
            program,
            models,
            &mut outlined,
            &mut digests,
            &native,
        )
    });
    let mut lowerings = lock(engine);
    lowerings.outlines = outlined;
    lowerings.digests = digests;
    lowerings.native = Some((registered, native));
    interface
}

impl ExecutableOp {
    /// Each adapter the op calls into `adapters`: its own, or a witnessed
    /// op's template's, which each instance calls.
    fn adapters<'o>(&'o self, adapters: &mut BTreeSet<&'o host::structural::Definition>) {
        match self {
            Self::Imported { proof } => {
                adapters.insert(proof.definition());
            }
            Self::Witnessed(op) => op.template.adapters(adapters),
            _ => {}
        }
    }

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

#[cfg(test)]
mod tests {
    use super::{adapters, Kept, Outside};
    use crate::host::{self, facade, structural};
    use crate::resolve::lower_nominal::entry::Lowered;
    use crate::resolve::lower_nominal::op::{ExecutableOp, WitnessedOp};
    use std::collections::BTreeSet;

    /// A kept part whose one op is a witnessed op over a template that
    /// calls `adapter`.
    fn witnessed(adapter: &structural::Definition) -> Kept {
        let proof = structural::Proof::transform(
            adapter.clone(),
            Box::default(),
            host::runtime::type_ref::Type::unit(),
            None,
            crate::Span::NONE,
        )
        .expect("an adapter of no operands takes none");
        let op = ExecutableOp::Witnessed(Box::new(WitnessedOp::new(
            ExecutableOp::Imported { proof },
            1,
        )));
        let lowered = [Lowered {
            source: String::new(),
            locations: Vec::new(),
            ops: vec![op],
            patterns: Vec::new(),
            closures: Vec::new(),
        }];
        Kept {
            key: [0; 32],
            adapters: adapters(&lowered),
            lowered: lowered.into(),
            uses: Box::default(),
            dependencies: BTreeSet::new(),
            outside: Outside::new(Vec::new(), Vec::new()),
            parsed: None,
        }
    }

    #[test]
    fn a_kept_witnessed_op_is_reused_only_while_the_runner_admits_its_template_s_adapter() {
        let definition = facade::Builder::new("structure", facade::Abi::new(1).unwrap())
            .unwrap()
            .finish()
            .unwrap();
        let owner = facade::Dependency::new(
            &host::path::Path::parse("structure").unwrap(),
            definition.fingerprint().clone(),
        );
        let mut engine = crate::Engine::new();
        engine.register_host_facade(definition).unwrap();
        let adapter = structural::Definition::new(
            owner,
            "structure::Cell".into(),
            [12; 32],
            [13; 32],
            [14; 32],
            0,
            [15; 32],
            structural::Kind::Field { copy: true },
        );
        let kept = witnessed(&adapter);
        assert!(
            !kept.admitted(&engine.host_registry),
            "a runner that lacks the adapter lowers the part again"
        );
        engine
            .host_registry
            .admit_structural(adapter, &mut host::TestImportAllocation::default())
            .unwrap();
        assert!(kept.admitted(&engine.host_registry));
    }
}
