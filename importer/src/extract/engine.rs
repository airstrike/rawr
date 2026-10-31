//! Extraction as a traversal over units of work that depend on the crate
//! alone (D609.3).
//!
//! The driver's extraction of a selected crate walks definitions, trait
//! obligations and implementations from the paths Rawr demands. Each step
//! of that walk, a unit, reads the compiler and produces emissions: more
//! work to schedule and the protocol values it found. A unit's emissions
//! are a function of the crate and the unit alone. Every decision that
//! depends on what else the walk reached is the engine's: which units run,
//! in which order, and how their values combine.
//!
//! So the driver answers units from the compiler and records them in the
//! crate's table, and the same engine replays any demand whose units the
//! table holds with no compiler at all. The two produce the same body,
//! because they are the same walk over the same answers. A small crate's
//! table holds its whole walk. A large crate's grows with each demand it
//! meets: walking iced whole cost the counter's first run about 18 s in a
//! debug build.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[path = "engine/home.rs"]
pub(crate) mod home;
#[path = "engine/model.rs"]
pub(crate) mod model;

use model::{
    Alias, Binding, Export, ExportTarget, Id, Implementation, Namespace, Reexport,
    ScalarPredicateProof, Span,
};

/// The format of a recorded unit table. A table of another format is not
/// read; it is extracted again.
pub(crate) const TABLE_SCHEMA: u32 = 4;

const MAX_DEFINITIONS: usize = 500_000;
const MAX_PACKAGES: usize = 4_096;
const MAX_EDGES: usize = 1_000_000;
const MAX_DEPTH: usize = 256;
/// A macro's `$crate`: the leading segment of a demanded path that names the
/// root of the crate the macro is defined in.
pub(crate) const CRATE_SEGMENT: &str = "__rawr_crate_";

/// A definition's def-path hash, unique across crates.
pub(crate) type Hash = [u8; 16];

/// A definition in one namespace: the order the walk takes work in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Key {
    #[serde(with = "hex16")]
    pub(crate) hash: Hash,
    pub(crate) namespace: Namespace,
}

/// How much of a definition the walk needs: its shape, or its shape with
/// its impl tables and public surface.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Level {
    Shape,
    Selected,
}

/// A request for a definition at a level, with its owner as the compiler
/// gives it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Request {
    pub(crate) key: Key,
    pub(crate) owner: Box<RawValue>,
    pub(crate) level: Level,
}

/// A type that an alias passing its parameters on names: where its crate's
/// root is, for naming it under that crate's `$crate` segment.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AliasTarget {
    pub(crate) target: Key,
    pub(crate) krate: String,
    pub(crate) selected: bool,
    pub(crate) sysroot: bool,
    #[serde(with = "hex16")]
    pub(crate) root: Hash,
    pub(crate) segment: String,
}

/// What one unit of work produced.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Emission {
    Inventory {
        inventory: model::Inventory,
    },
    InherentInventory {
        inventory: model::Inherent,
    },
    /// Schedule a definition. A struct or enum with an alias that passes its
    /// parameters on asks for the alias's shape first, when the alias is not
    /// scheduled yet.
    Request {
        request: Request,
        alias: Option<Request>,
    },
    /// A trait obligation, selected when the walk is at a selected root.
    Obligation {
        key: String,
        class: Class,
    },
    /// An implementation, relevant to `owner`, with an associated member
    /// obligation.
    Implementation {
        #[serde(with = "hex16")]
        implementation: Hash,
        #[serde(with = "hex16")]
        owner: Hash,
        associated: Option<String>,
    },
    /// A definition's protocol value. A type alias says what type it passes
    /// its parameters on to.
    Definition {
        key: Key,
        id: Id,
        value: Box<RawValue>,
        alias: Option<AliasTarget>,
    },
    /// A crate the extraction reached.
    Register {
        sysroot: bool,
        crate_name: String,
        stable_crate_id: String,
    },
    /// An implementation's fixed fields; a reservation implementation has
    /// none.
    Base {
        implementation: Option<Box<Implementation>>,
    },
    /// Definitions an implementation is relevant to.
    Relevance {
        ids: Vec<Id>,
    },
    /// An implementation's selected associated definitions.
    Associated {
        ids: Vec<Id>,
    },
    /// A blanket implementation applied to one concrete type.
    Application {
        implementation: Box<Implementation>,
    },
    /// A scalar predicate's outcome.
    Scalar {
        key: String,
        proof: ScalarPredicateProof,
    },
    /// The names a struct or enum's inherent impls give.
    Inherent {
        names: Vec<String>,
    },
}

/// What a trait obligation's self type is, which decides whether being
/// selected matters to it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Class {
    /// A concrete type: its candidates, selected or not.
    Concrete,
    /// A generic parameter: its relations and scalar proofs, when selected;
    /// nothing otherwise.
    Parameter,
    /// A bound variable, placeholder, inference variable or alias: nothing.
    Inert,
}

/// One unit of work: everything it emits depends on the crate and the unit.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Unit {
    /// All trait implementations owned by this crate, with full members.
    Inventory(#[serde(with = "hex16")] Hash),
    InventoryShape(Key),
    /// A definition's shape.
    Shape(Key),
    /// A selected definition's impl tables.
    Implementations(#[serde(with = "hex16")] Hash),
    /// A selected definition's public surface.
    Surface(#[serde(with = "hex16")] Hash),
    /// A trait obligation's candidate implementations: those of a concrete
    /// self type, or a selected parameter's relations.
    Obligation {
        key: String,
    },
    /// An implementation's own fields and their requests.
    Base(#[serde(with = "hex16")] Hash),
    /// An implementation's relevance to a requesting definition.
    Owner {
        #[serde(with = "hex16")]
        implementation: Hash,
        #[serde(with = "hex16")]
        owner: Hash,
    },
    /// An implementation's associated members an obligation selects.
    Associated {
        #[serde(with = "hex16")]
        implementation: Hash,
        obligation: String,
    },
    /// A demanded `Type::member` that a trait provides. The traits the
    /// compiler sees are every selected crate's, so the unit is keyed by
    /// them too (`session`).
    Member {
        #[serde(with = "hex16")]
        owner: Hash,
        name: String,
        session: String,
    },
    /// A demanded `Trait::member`, for the trait's impls on `&str` in every
    /// selected crate (`session`).
    TraitMember {
        #[serde(with = "hex16")]
        owner: Hash,
        name: String,
        session: String,
    },
}

/// One public child of a module, as both walks read it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Child {
    pub(crate) name: String,
    /// What a demanded path landing here selects, when the child is a
    /// definition with a namespace; an error the compiler raised for it.
    pub(crate) landing: Option<Result<Landing, String>>,
    /// What a public walk names here; `None` for a variant's constructor.
    pub(crate) export: Option<ChildExport>,
}

/// A demanded path's segment landing on a definition.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Landing {
    pub(crate) requests: Vec<(Request, Option<Request>)>,
    pub(crate) next: Next,
}

/// Where a demanded path goes after a segment.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum Next {
    Module(#[serde(with = "hex16")] Hash),
    Nominal(#[serde(with = "hex16")] Hash),
    Trait(#[serde(with = "hex16")] Hash),
    Other,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ChildExport {
    pub(crate) binding: Binding,
    pub(crate) variant: Option<String>,
    pub(crate) span: Span,
    pub(crate) reexports: Vec<Reexport>,
    /// Re-export hops through crates other than the selected one.
    pub(crate) hops: Vec<Crate>,
    pub(crate) target: ChildTarget,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) enum ChildTarget {
    Definition {
        key: Key,
        id: Id,
        /// Raised when the child is walked: an anonymous associated item or
        /// an anonymous import without trait evidence.
        error: Option<String>,
        display_path: Vec<String>,
        /// The request a public walk makes, at the level its binding gives.
        request: Box<Result<(Request, Option<Request>), String>>,
        /// A nested module, and whether it is another crate's root.
        module: Option<Nested>,
    },
    /// A definition with no namespace.
    Unnamespaced {
        error: String,
    },
    Primitive {
        compiler_kind: String,
    },
    Other {
        error: String,
    },
}

/// A nested module, and whether it is another crate's root.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub(crate) struct Nested {
    #[serde(with = "hex16")]
    pub(crate) module: Hash,
    pub(crate) foreign: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Crate {
    pub(crate) sysroot: bool,
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
}

/// A crate's root as a macro's `$crate` names it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Segment {
    #[serde(with = "hex16")]
    pub(crate) root: Hash,
    pub(crate) krate: String,
}

/// Why a unit had no answer.
#[derive(Debug)]
pub(crate) enum Fault {
    /// The recorded table does not hold it; only the compiler can answer.
    Missing(String),
    Error(String),
}

impl From<String> for Fault {
    fn from(error: String) -> Self {
        Self::Error(error)
    }
}

/// Answers units: the compiler in the driver, a recorded table elsewhere.
pub(crate) trait Oracle {
    fn unit(&mut self, unit: &Unit) -> Result<Rc<[Emission]>, Fault>;
    fn module(&mut self, module: Hash) -> Result<Rc<[Child]>, Fault>;
    fn segment(&mut self, name: &str) -> Result<Option<Segment>, Fault>;
    fn index(&mut self) -> Result<Rc<[Box<RawValue>]>, Fault>;
}

/// The selected crate a walk starts from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Root {
    #[serde(with = "hex16")]
    pub(crate) module: Hash,
    pub(crate) krate: String,
}

/// Everything units answered for one crate, kept to be replayed.
#[derive(Default)]
pub(crate) struct Memo {
    pub(crate) whole: bool,
    /// The crate's whole walk passed the driver's bound, so its table grows
    /// with demand and the walk is not tried again.
    pub(crate) large: bool,
    pub(crate) units: BTreeMap<Unit, Rc<[Emission]>>,
    pub(crate) modules: BTreeMap<Hash, Rc<[Child]>>,
    pub(crate) segments: BTreeMap<String, Option<Segment>>,
    pub(crate) index: Option<Rc<[Box<RawValue>]>>,
}

/// One module's public children.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Module {
    #[serde(with = "hex16")]
    pub(crate) module: Hash,
    pub(crate) children: Vec<Child>,
}

/// A memo as it is written to disk.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Table {
    pub(crate) schema: u32,
    /// The nightly artifact the recorded answers came from. This validates
    /// local reuse; it is not part of the machine-wide crate key.
    pub(crate) crate_hash: String,
    pub(crate) root: Root,
    pub(crate) whole: bool,
    pub(crate) large: bool,
    pub(crate) units: Vec<(Unit, Vec<Emission>)>,
    pub(crate) modules: Vec<Module>,
    pub(crate) segments: Vec<(String, Option<Segment>)>,
    pub(crate) index: Option<Vec<Box<RawValue>>>,
}

impl Memo {
    pub(crate) fn from_table(table: Table) -> Self {
        Self {
            whole: table.whole,
            large: table.large,
            units: table
                .units
                .into_iter()
                .map(|(unit, emissions)| (unit, Rc::from(emissions)))
                .collect(),
            modules: table
                .modules
                .into_iter()
                .map(|module| (module.module, Rc::from(module.children)))
                .collect(),
            segments: table.segments.into_iter().collect(),
            index: table.index.map(Rc::from),
        }
    }

    pub(crate) fn to_table(&self, root: Root) -> Table {
        Table {
            schema: TABLE_SCHEMA,
            crate_hash: String::new(),
            root,
            whole: self.whole,
            large: self.large,
            units: self
                .units
                .iter()
                .map(|(unit, emissions)| (unit.clone(), emissions.to_vec()))
                .collect(),
            modules: self
                .modules
                .iter()
                .map(|(module, children)| Module {
                    module: *module,
                    children: children.to_vec(),
                })
                .collect(),
            segments: self
                .segments
                .iter()
                .map(|(name, segment)| (name.clone(), segment.clone()))
                .collect(),
            index: self.index.as_ref().map(|index| index.to_vec()),
        }
    }

    /// Take every answer `other` holds that this memo lacks.
    pub(crate) fn absorb(&mut self, other: &Self) {
        self.whole |= other.whole;
        self.large |= other.large;
        for (unit, emissions) in &other.units {
            self.units
                .entry(unit.clone())
                .or_insert_with(|| Rc::clone(emissions));
        }
        for (module, children) in &other.modules {
            self.modules
                .entry(*module)
                .or_insert_with(|| Rc::clone(children));
        }
        for (name, segment) in &other.segments {
            self.segments
                .entry(name.clone())
                .or_insert_with(|| segment.clone());
        }
        if self.index.is_none() {
            self.index.clone_from(&other.index);
        }
    }
}

/// Answers only from a memo: a unit it lacks is missing.
pub(crate) struct Replay<'memo>(pub(crate) &'memo Memo);

impl Oracle for Replay<'_> {
    fn unit(&mut self, unit: &Unit) -> Result<Rc<[Emission]>, Fault> {
        self.0
            .units
            .get(unit)
            .cloned()
            .ok_or_else(|| Fault::Missing(format!("{unit:?}")))
    }

    fn module(&mut self, module: Hash) -> Result<Rc<[Child]>, Fault> {
        self.0
            .modules
            .get(&module)
            .cloned()
            .ok_or_else(|| Fault::Missing(format!("module {}", hex(&module))))
    }

    fn segment(&mut self, name: &str) -> Result<Option<Segment>, Fault> {
        self.0
            .segments
            .get(name)
            .cloned()
            .ok_or_else(|| Fault::Missing(format!("segment {name}")))
    }

    fn index(&mut self) -> Result<Rc<[Box<RawValue>]>, Fault> {
        self.0
            .index
            .clone()
            .ok_or_else(|| Fault::Missing("name index".to_owned()))
    }
}

/// Answers from a memo first, and records what `live` answers.
pub(crate) struct Record<'a, O: Oracle> {
    pub(crate) live: &'a mut O,
    pub(crate) memo: &'a mut Memo,
}

impl<O: Oracle> Oracle for Record<'_, O> {
    fn unit(&mut self, unit: &Unit) -> Result<Rc<[Emission]>, Fault> {
        if let Some(emissions) = self.memo.units.get(unit) {
            return Ok(Rc::clone(emissions));
        }
        let emissions = self.live.unit(unit)?;
        self.memo.units.insert(unit.clone(), Rc::clone(&emissions));
        Ok(emissions)
    }

    fn module(&mut self, module: Hash) -> Result<Rc<[Child]>, Fault> {
        if let Some(children) = self.memo.modules.get(&module) {
            return Ok(Rc::clone(children));
        }
        let children = self.live.module(module)?;
        self.memo.modules.insert(module, Rc::clone(&children));
        Ok(children)
    }

    fn segment(&mut self, name: &str) -> Result<Option<Segment>, Fault> {
        if let Some(segment) = self.memo.segments.get(name) {
            return Ok(segment.clone());
        }
        let segment = self.live.segment(name)?;
        self.memo.segments.insert(name.to_owned(), segment.clone());
        Ok(segment)
    }

    fn index(&mut self) -> Result<Rc<[Box<RawValue>]>, Fault> {
        if let Some(index) = &self.memo.index {
            return Ok(Rc::clone(index));
        }
        let index = self.live.index()?;
        self.memo.index = Some(Rc::clone(&index));
        Ok(index)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Queued,
    Active,
    Complete(Level),
}

struct Scheduled {
    owner: Box<RawValue>,
    level: Level,
    state: State,
}

struct Defined {
    id: Id,
    value: Box<RawValue>,
    alias: Option<AliasTarget>,
}

struct Pending {
    class: Class,
    selected: bool,
    complete: bool,
}

#[derive(Default)]
struct Requested {
    owners: Vec<Hash>,
    associated: BTreeSet<String>,
}

struct ForeignRoot {
    module: Hash,
    path: Vec<String>,
    reexports: Vec<Reexport>,
    depth: usize,
}

/// What one walk produced.
pub(crate) struct Extracted {
    pub(crate) reachable: Vec<Export>,
    definitions: Vec<Defined>,
    implementations: Vec<Implementation>,
    scalar_predicate_proofs: Vec<ScalarPredicateProof>,
    index: Rc<[Box<RawValue>]>,
    /// The crates it reached, sysroot and external, in order.
    pub(crate) crates: BTreeSet<Crate>,
    inventory: Option<model::Inventory>,
}

impl Extracted {
    /// The protocol body, exactly as the sidecar carries it.
    pub(crate) fn body(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&model::Body {
            reachable: &self.reachable,
            definitions: self
                .definitions
                .iter()
                .map(|definition| &*definition.value)
                .collect(),
            implementations: self.implementations.iter().collect(),
            scalar_predicate_proofs: self.scalar_predicate_proofs.iter().collect(),
            index: &self.index,
            inventory: self.inventory.as_ref(),
        })
        .map_err(|error| format!("failed to encode compiler sidecar: {error}"))
    }
}

/// One walk of a selected crate.
pub(crate) struct Engine<'o, O: Oracle> {
    oracle: &'o mut O,
    root: Root,
    aliases: Vec<String>,
    session: String,
    inventory: Option<model::Inventory>,
    scheduler: BTreeMap<Key, Scheduled>,
    ready: BTreeSet<Key>,
    definitions: BTreeMap<Key, Defined>,
    implemented: BTreeSet<Hash>,
    surfaced: BTreeSet<Hash>,
    obligations: BTreeMap<String, Pending>,
    queued: usize,
    draining: bool,
    requests: BTreeMap<Hash, Requested>,
    implementations: BTreeMap<Hash, Implementation>,
    applications: BTreeMap<String, Implementation>,
    scalars: BTreeMap<String, ScalarPredicateProof>,
    crates: BTreeSet<Crate>,
    external: BTreeSet<String>,
    reachable: Vec<Export>,
    foreign_roots: Vec<ForeignRoot>,
    crate_roots: Vec<(Hash, String, String)>,
    members: Vec<(Hash, String)>,
    trait_members: Vec<(Hash, String)>,
    inherent: BTreeMap<Hash, Vec<String>>,
    /// The definition whose impl tables are being applied.
    current: Option<Hash>,
    active: Level,
    only_names: Option<BTreeSet<Key>>,
}

impl<'o, O: Oracle> Engine<'o, O> {
    /// A walk of `root`'s crate under `aliases`, in a session of selected
    /// crates `session` names (`session_key`).
    pub(crate) fn new(
        oracle: &'o mut O,
        root: Root,
        aliases: Vec<String>,
        session: String,
    ) -> Self {
        Self {
            oracle,
            root,
            aliases,
            session,
            inventory: None,
            scheduler: BTreeMap::new(),
            ready: BTreeSet::new(),
            definitions: BTreeMap::new(),
            implemented: BTreeSet::new(),
            surfaced: BTreeSet::new(),
            obligations: BTreeMap::new(),
            queued: 0,
            draining: false,
            requests: BTreeMap::new(),
            implementations: BTreeMap::new(),
            applications: BTreeMap::new(),
            scalars: BTreeMap::new(),
            crates: BTreeSet::new(),
            external: BTreeSet::new(),
            reachable: Vec::new(),
            foreign_roots: Vec::new(),
            crate_roots: Vec::new(),
            members: Vec::new(),
            trait_members: Vec::new(),
            inherent: BTreeMap::new(),
            current: None,
            active: Level::Shape,
            only_names: None,
        }
    }

    /// Walk the crate for `demand`, its paths with the crate's alias first;
    /// an empty demand is the whole crate.
    pub(crate) fn run(mut self, demand: &[Vec<String>]) -> Result<Extracted, Fault> {
        let root = self.root.module;
        // A complete table must also answer a later named demand. Capture
        // the index on whole-crate walks, not only on already-demanded ones.
        let index = self.oracle.index()?;
        if demand.is_empty() {
            // Whole-crate seeding: every public path of the selected crate.
            self.walk(root, Vec::new(), &[], 0, false)?;
            self.drain()?;
            for foreign in std::mem::take(&mut self.foreign_roots) {
                self.walk(
                    foreign.module,
                    foreign.path,
                    &foreign.reexports,
                    foreign.depth,
                    true,
                )?;
            }
        } else {
            for path in demand {
                self.select_path(root, path.get(1..).unwrap_or_default(), 0)?;
            }
            self.drain()?;
            self.resolve_members()?;
            self.drain()?;
            self.walk(root, Vec::new(), &[], 0, true)?;
            let hidden = self.hidden_alias_targets();
            // A macro's `$crate` paths, under the segment naming its crate.
            for (module, segment, _) in std::mem::take(&mut self.crate_roots) {
                self.walk(module, vec![segment], &[], 1, true)?;
            }
            // Each type only an alias names, under the segment naming its
            // crate; nothing else of that crate.
            if !hidden.is_empty() {
                let mut roots = hidden
                    .iter()
                    .map(|target| (target.root, target.segment.clone()))
                    .collect::<Vec<_>>();
                roots.sort_unstable();
                roots.dedup();
                self.only_names = Some(hidden.iter().map(|target| target.target).collect());
                for (module, segment) in roots {
                    self.walk(module, vec![segment], &[], 1, true)?;
                }
                self.only_names = None;
            }
        }
        let mut definitions = std::mem::take(&mut self.definitions)
            .into_values()
            .collect::<Vec<_>>();
        definitions.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        let mut implementations = std::mem::take(&mut self.implementations)
            .into_values()
            .chain(std::mem::take(&mut self.applications).into_values())
            .collect::<Vec<_>>();
        implementations.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        let mut scalar_predicate_proofs = std::mem::take(&mut self.scalars)
            .into_values()
            .collect::<Vec<_>>();
        scalar_predicate_proofs.sort_unstable();
        let mut reachable = std::mem::take(&mut self.reachable);
        reachable.sort_unstable();
        if let Some(inventory) = &mut self.inventory {
            inventory.inherent.sort_unstable();
        }
        Ok(Extracted {
            reachable,
            definitions,
            implementations,
            scalar_predicate_proofs,
            index,
            crates: std::mem::take(&mut self.crates),
            inventory: self.inventory,
        })
    }

    /// A complete intrinsic domain never executes contextual method,
    /// obligation, or surface units. Raw shapes retain their full predicates.
    pub(crate) fn inventory(mut self) -> Result<Extracted, Fault> {
        let emissions = self.oracle.unit(&Unit::Inventory(self.root.module))?;
        let mut manifest = None;
        for emission in emissions.iter() {
            if let Emission::Inventory { inventory } = emission
                && manifest.replace(inventory.clone()).is_some()
            {
                return Err(Fault::Error(
                    "duplicate intrinsic completion manifest".into(),
                ));
            }
        }
        self.inventory = Some(
            manifest.ok_or_else(|| Fault::Error("missing intrinsic completion manifest".into()))?,
        );
        self.apply_emissions(&emissions)?;
        self.run(&[])
    }

    fn apply(&mut self, unit: &Unit) -> Result<Vec<Id>, Fault> {
        let emissions = self.oracle.unit(unit)?;
        self.apply_emissions(&emissions)
    }

    /// Apply a unit's emissions; the ids an implementation's relevance and
    /// associated emissions name, in order.
    fn apply_emissions(&mut self, emissions: &[Emission]) -> Result<Vec<Id>, Fault> {
        let mut ids = Vec::new();
        for emission in emissions.iter() {
            match emission {
                Emission::Request { request, alias } => self.request(request, alias.as_ref())?,
                Emission::Obligation { key, class } if self.inventory.is_none() => {
                    self.obligation(key, *class)?
                }
                Emission::Obligation { .. } | Emission::Inventory { .. } => {}
                Emission::InherentInventory { inventory } => {
                    self.inventory
                        .as_mut()
                        .ok_or_else(|| {
                            Fault::Error("intrinsic domain outside inventory acquisition".into())
                        })?
                        .inherent
                        .push(inventory.clone());
                }
                Emission::Implementation {
                    implementation,
                    owner,
                    associated,
                } => {
                    let requested = self.requests.entry(*implementation).or_default();
                    if !requested.owners.contains(owner) {
                        requested.owners.push(*owner);
                    }
                    if let Some(associated) = associated {
                        requested.associated.insert(associated.clone());
                    }
                }
                Emission::Definition {
                    key,
                    id,
                    value,
                    alias,
                } => {
                    if self.definitions.contains_key(key) {
                        return Err(Fault::Error(
                            "duplicate external definition identity".to_owned(),
                        ));
                    }
                    if self.definitions.len() >= MAX_DEFINITIONS {
                        return Err(Fault::Error(
                            "compiler definition limit exceeded".to_owned(),
                        ));
                    }
                    self.definitions.insert(
                        *key,
                        Defined {
                            id: id.clone(),
                            value: value.clone(),
                            alias: alias.clone(),
                        },
                    );
                }
                Emission::Register {
                    sysroot,
                    crate_name,
                    stable_crate_id,
                } => self.register(Crate {
                    sysroot: *sysroot,
                    crate_name: crate_name.clone(),
                    stable_crate_id: stable_crate_id.clone(),
                })?,
                Emission::Relevance { ids: values } | Emission::Associated { ids: values } => {
                    ids.extend(values.iter().cloned());
                }
                Emission::Base { implementation } => {
                    if self.inventory.is_some()
                        && let Some(implementation) = implementation
                        && self
                            .implementations
                            .insert(
                                parse_hash(&implementation.id.def_path_hash)?,
                                (**implementation).clone(),
                            )
                            .is_some()
                    {
                        return Err(Fault::Error("duplicate intrinsic implementation".into()));
                    }
                }
                Emission::Inherent { names } => {
                    if let Some(hash) = self.current {
                        self.inherent.insert(hash, names.clone());
                    }
                }
                Emission::Application { implementation } => {
                    match self.applications.get_mut(&implementation.key) {
                        Some(existing) => {
                            existing
                                .associated
                                .extend(implementation.associated.iter().cloned());
                            existing.associated.sort_unstable();
                            existing.associated.dedup();
                        }
                        None => {
                            self.applications
                                .insert(implementation.key.clone(), (**implementation).clone());
                        }
                    }
                }
                Emission::Scalar { key, proof } => match self.scalars.get(key) {
                    Some(existing) if existing != proof => {
                        return Err(Fault::Error(
                            "scalar predicate proof identity collision".to_owned(),
                        ));
                    }
                    Some(_) => {}
                    None => {
                        self.scalars.insert(key.clone(), proof.clone());
                    }
                },
            }
        }
        Ok(ids)
    }

    fn register(&mut self, krate: Crate) -> Result<(), Fault> {
        if !krate.sysroot && !self.external.contains(&krate.stable_crate_id) {
            if self.external.len() >= MAX_PACKAGES {
                return Err(Fault::Error(
                    "external compiler crate limit exceeded".to_owned(),
                ));
            }
            self.external.insert(krate.stable_crate_id.clone());
        }
        self.crates.insert(krate);
        Ok(())
    }

    fn request(&mut self, request: &Request, alias: Option<&Request>) -> Result<(), Fault> {
        if let Some(alias) = alias
            && !self.scheduler.contains_key(&alias.key)
        {
            self.request_one(alias)?;
        }
        self.request_one(request)
    }

    fn request_one(&mut self, request: &Request) -> Result<(), Fault> {
        let level = if self.inventory.is_some() {
            Level::Shape
        } else {
            request.level
        };
        match self.scheduler.get_mut(&request.key) {
            Some(entry) => {
                if entry.owner.get() != request.owner.get() {
                    return Err(Fault::Error(
                        "stable definition owner traversal identity collision".to_owned(),
                    ));
                }
                entry.level = entry.level.max(level);
                if let State::Complete(completed) = entry.state
                    && entry.level > completed
                {
                    entry.state = State::Queued;
                    self.ready.insert(request.key);
                }
            }
            None => {
                if self.scheduler.len() >= MAX_DEFINITIONS {
                    return Err(Fault::Error(
                        "compiler definition limit exceeded".to_owned(),
                    ));
                }
                self.scheduler.insert(
                    request.key,
                    Scheduled {
                        owner: request.owner.clone(),
                        level,
                        state: State::Queued,
                    },
                );
                self.ready.insert(request.key);
            }
        }
        Ok(())
    }

    fn obligation(&mut self, key: &str, class: Class) -> Result<(), Fault> {
        if self.draining {
            return Err(Fault::Error(
                "trait obligation traversal recursively enqueued work".to_owned(),
            ));
        }
        let selected = self.active == Level::Selected;
        match self.obligations.get_mut(key) {
            Some(existing) => {
                if selected {
                    existing.selected = true;
                }
            }
            None => {
                self.obligations.insert(
                    key.to_owned(),
                    Pending {
                        class,
                        selected,
                        complete: false,
                    },
                );
                self.queued += 1;
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), Fault> {
        loop {
            if let Some(key) = self.ready.pop_first() {
                let entry = self.scheduler.get_mut(&key).ok_or_else(|| {
                    Fault::Error("traversal scheduler lost a queued identity".to_owned())
                })?;
                if entry.state != State::Queued {
                    return Err(Fault::Error(
                        "traversal scheduler selected a non-queued identity".to_owned(),
                    ));
                }
                entry.state = State::Active;
                let level = entry.level;
                let previous = self.active;
                self.active = level;
                let result = self.process(key, level);
                self.active = previous;
                result?;
                let entry = self.scheduler.get_mut(&key).ok_or_else(|| {
                    Fault::Error("traversal scheduler lost an active identity".to_owned())
                })?;
                if entry.level > level {
                    entry.state = State::Queued;
                    self.ready.insert(key);
                } else {
                    entry.state = State::Complete(level);
                }
                continue;
            }
            if self.queued != 0 {
                self.drain_obligations()?;
                continue;
            }
            if let Some((implementation, requested)) = self.requests.pop_first() {
                self.implementation(implementation, requested)?;
                continue;
            }
            break;
        }
        Ok(())
    }

    fn process(&mut self, key: Key, level: Level) -> Result<(), Fault> {
        if !self.definitions.contains_key(&key) {
            self.apply(&if self.inventory.is_some() {
                Unit::InventoryShape(key)
            } else {
                Unit::Shape(key)
            })?;
        }
        if level == Level::Selected {
            if self.implemented.insert(key.hash) {
                self.current = Some(key.hash);
                let applied = self.apply(&Unit::Implementations(key.hash));
                self.current = None;
                applied?;
            }
            if self.surfaced.insert(key.hash) {
                self.apply(&Unit::Surface(key.hash))?;
            }
        }
        Ok(())
    }

    fn drain_obligations(&mut self) -> Result<(), Fault> {
        let pending = self
            .obligations
            .iter()
            .filter(|(_, pending)| !pending.complete)
            .map(|(key, pending)| (key.clone(), pending.class, pending.selected))
            .collect::<Vec<_>>();
        self.draining = true;
        let mut result = Ok(());
        for (key, class, selected) in pending {
            // An unselected parameter, or an inert self type, has no
            // candidates to read: the unit would emit nothing.
            let work = match class {
                Class::Concrete => true,
                Class::Parameter => selected,
                Class::Inert => false,
            };
            if work && let Err(error) = self.apply(&Unit::Obligation { key: key.clone() }) {
                result = Err(error);
                break;
            }
            if let Some(pending) = self.obligations.get_mut(&key) {
                pending.complete = true;
            }
            self.queued = self.queued.saturating_sub(1);
        }
        self.draining = false;
        result
    }

    fn implementation(&mut self, implementation: Hash, requested: Requested) -> Result<(), Fault> {
        let mut relevance = Vec::new();
        let mut associated = Vec::new();
        let existing = self.implementations.remove(&implementation);
        let mut record = match existing {
            Some(existing) => Some(existing),
            None => {
                let emissions = self.oracle.unit(&Unit::Base(implementation))?;
                let base = emissions.iter().find_map(|emission| match emission {
                    Emission::Base { implementation } => Some(implementation.clone()),
                    _ => None,
                });
                let base = base.ok_or_else(|| {
                    Fault::Error("an implementation unit carried no base".to_owned())
                })?;
                self.apply_emissions(&emissions)?;
                base.map(|base| *base)
            }
        };
        for owner in &requested.owners {
            relevance.extend(self.apply(&Unit::Owner {
                implementation,
                owner: *owner,
            })?);
        }
        for obligation in &requested.associated {
            let emissions = self.oracle.unit(&Unit::Associated {
                implementation,
                obligation: obligation.clone(),
            })?;
            for emission in emissions.iter() {
                match emission {
                    Emission::Relevance { ids } => relevance.extend(ids.iter().cloned()),
                    Emission::Associated { ids } => associated.extend(ids.iter().cloned()),
                    _ => {}
                }
            }
            self.apply_emissions(&emissions)?;
        }
        if let Some(record) = &mut record {
            if record.relevance.len().saturating_add(relevance.len()) > MAX_EDGES {
                return Err(Fault::Error(
                    "implementation relevance edge limit exceeded".to_owned(),
                ));
            }
            record.relevance.extend(relevance);
            record.relevance.sort_unstable();
            record.relevance.dedup();
            if record.associated.len().saturating_add(associated.len()) > MAX_EDGES {
                return Err(Fault::Error(
                    "implementation associated edge limit exceeded".to_owned(),
                ));
            }
            record.associated.extend(associated);
            record.associated.sort_unstable();
            record.associated.dedup();
        }
        if let Some(record) = record {
            self.implementations.insert(implementation, record);
        }
        Ok(())
    }

    fn resolve_members(&mut self) -> Result<(), Fault> {
        let mut members = std::mem::take(&mut self.members);
        members.sort_unstable();
        members.dedup();
        for (owner, name) in members {
            // An inherent member needs no trait: the unit would emit nothing.
            if self
                .inherent
                .get(&owner)
                .is_some_and(|names| names.binary_search(&name).is_ok())
            {
                continue;
            }
            let session = self.session.clone();
            self.apply(&Unit::Member {
                owner,
                name,
                session,
            })?;
        }
        let mut members = std::mem::take(&mut self.trait_members);
        members.sort_unstable();
        members.dedup();
        for (owner, name) in members {
            let session = self.session.clone();
            self.apply(&Unit::TraitMember {
                owner,
                name,
                session,
            })?;
        }
        Ok(())
    }

    /// Select what one demanded path names, as the driver's `select_path`
    /// documents.
    fn select_path(&mut self, module: Hash, path: &[String], depth: usize) -> Result<(), Fault> {
        if depth > MAX_DEPTH {
            return Err(Fault::Error(
                "demanded path traversal depth exceeded".to_owned(),
            ));
        }
        let Some((name, rest)) = path.split_first() else {
            return Ok(());
        };
        if depth == 0 && name.starts_with(CRATE_SEGMENT) {
            if let Some(segment) = self.oracle.segment(name)? {
                if !self
                    .crate_roots
                    .iter()
                    .any(|(known, _, _)| *known == segment.root)
                {
                    self.crate_roots
                        .push((segment.root, name.clone(), segment.krate));
                }
                return self.select_path(segment.root, rest, depth.saturating_add(1));
            }
            return Ok(());
        }
        let children = self.oracle.module(module)?;
        for child in children.iter() {
            if name != "*" && child.name != *name {
                continue;
            }
            let Some(landing) = &child.landing else {
                continue;
            };
            let landing = landing
                .as_ref()
                .map_err(|error| Fault::Error(error.clone()))?;
            for (request, alias) in &landing.requests {
                self.request(request, alias.as_ref())?;
            }
            match (&landing.next, rest) {
                (Next::Module(module), rest) if !rest.is_empty() => {
                    self.select_path(*module, rest, depth.saturating_add(1))?;
                }
                (Next::Nominal(owner), [member]) if member != "*" => {
                    self.members.push((*owner, member.clone()));
                }
                (Next::Trait(owner), [member]) if member != "*" => {
                    self.trait_members.push((*owner, member.clone()));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Walk one public module, as the driver's `collect_external_exports`
    /// documents. With `names_only`, the walk extracts nothing: it names only
    /// definitions the extraction already holds.
    fn walk(
        &mut self,
        module: Hash,
        prefix: Vec<String>,
        inherited: &[Reexport],
        depth: usize,
        names_only: bool,
    ) -> Result<(), Fault> {
        if depth > MAX_DEPTH {
            return Err(Fault::Error(
                "external public module traversal depth exceeded".to_owned(),
            ));
        }
        let children = self.oracle.module(module)?;
        for child in children.iter() {
            let Some(export) = &child.export else {
                continue;
            };
            if names_only {
                match &export.target {
                    ChildTarget::Definition {
                        module: Some(_), ..
                    } => {}
                    ChildTarget::Definition { key, .. }
                        if self
                            .only_names
                            .as_ref()
                            .is_some_and(|only| !only.contains(key)) =>
                    {
                        continue;
                    }
                    ChildTarget::Definition { key, .. } if self.definitions.contains_key(key) => {}
                    _ => continue,
                }
            }
            let mut path = prefix.clone();
            path.push(child.name.clone());
            let (namespace, target, aliases, nested, named) = match &export.target {
                ChildTarget::Definition {
                    key,
                    id,
                    error,
                    display_path,
                    request,
                    module,
                } => {
                    if let Some(error) = error {
                        return Err(Fault::Error(error.clone()));
                    }
                    let named = !names_only || self.definitions.contains_key(key);
                    if !names_only {
                        let (request, alias) = request
                            .as_ref()
                            .as_ref()
                            .map_err(|error| Fault::Error(error.clone()))?;
                        self.request(request, alias.as_ref())?;
                    }
                    let aliases = if *display_path == path {
                        Vec::new()
                    } else {
                        vec![Alias {
                            from: display_path.clone(),
                            to: path.clone(),
                            span: export.span.clone(),
                        }]
                    };
                    (
                        key.namespace,
                        ExportTarget::Definition { id: id.clone() },
                        aliases,
                        *module,
                        named,
                    )
                }
                ChildTarget::Primitive { compiler_kind } => (
                    Namespace::Type,
                    ExportTarget::Unsupported {
                        compiler_kind: compiler_kind.clone(),
                    },
                    Vec::new(),
                    None,
                    true,
                ),
                ChildTarget::Unnamespaced { error } | ChildTarget::Other { error } => {
                    return Err(Fault::Error(error.clone()));
                }
            };
            if export.binding == Binding::AnonymousImport
                && !matches!(target, ExportTarget::Definition { .. })
            {
                return Err(Fault::Error(
                    "anonymous import target is not a definition".to_owned(),
                ));
            }
            let mut reexports = inherited.to_vec();
            if names_only {
                for hop in &export.hops {
                    self.register(hop.clone())?;
                }
            }
            reexports.extend(export.reexports.iter().cloned());
            if named {
                for root_alias in &self.aliases {
                    self.reachable.push(Export {
                        binding: export.binding,
                        root_alias: root_alias.clone(),
                        path: path.clone(),
                        namespace,
                        target: target.clone(),
                        aliases: aliases.clone(),
                        reexports: reexports.clone(),
                        span: export.span.clone(),
                        variant: export.variant.clone(),
                    });
                }
            }
            let depth = depth.saturating_add(1);
            match nested {
                // A re-exported foreign crate root such as `pub use wgpu;`
                // is not extracted; it is only named after extraction drains.
                Some(Nested {
                    module,
                    foreign: true,
                }) if !names_only => {
                    self.foreign_roots.push(ForeignRoot {
                        module,
                        path,
                        reexports,
                        depth,
                    });
                }
                Some(Nested { module, .. }) => {
                    self.walk(module, path, &reexports, depth, names_only)?;
                }
                None => {}
            }
        }
        Ok(())
    }

    /// The types exported aliases pass their parameters on to that no
    /// public path of the root reaches, as the driver's
    /// `hidden_alias_targets` documents.
    fn hidden_alias_targets(&self) -> Vec<AliasTarget> {
        let exported = self
            .reachable
            .iter()
            .filter_map(|export| match &export.target {
                ExportTarget::Definition { id } => Some(id.def_path_hash.as_str()),
                ExportTarget::Unsupported { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let mut targets = Vec::new();
        for (key, definition) in &self.definitions {
            let Some(target) = &definition.alias else {
                continue;
            };
            if key.namespace != Namespace::Type || !exported.contains(hex(&key.hash).as_str()) {
                continue;
            }
            if !target.selected
                && self.definitions.contains_key(&target.target)
                && !exported.contains(hex(&target.target.hash).as_str())
                && !self
                    .crate_roots
                    .iter()
                    .any(|(_, _, krate)| *krate == target.krate)
                && !target.sysroot
            {
                targets.push(target.clone());
            }
        }
        targets
    }
}

/// What names a session's selected crates: their unit keys, in any order.
pub(crate) fn session_key<'a>(unit_keys: impl IntoIterator<Item = &'a str>) -> String {
    use sha2::{Digest as _, Sha256};
    let mut keys = unit_keys.into_iter().collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    let mut digest = Sha256::new();
    for key in keys {
        digest.update((key.len() as u64).to_le_bytes());
        digest.update(key.as_bytes());
    }
    hex(&digest.finalize())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    text
}

fn parse_hash(text: &str) -> Result<Hash, Fault> {
    let mut bytes = [0; 16];
    if text.len() != 32 || !text.is_ascii() {
        return Err(Fault::Error("a hash is 32 hexadecimal digits".into()));
    }
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|error| Fault::Error(error.to_string()))?;
    }
    Ok(bytes)
}

mod hex16 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer>(
        bytes: &[u8; 16],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&super::hex(bytes))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[u8; 16], D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut bytes = [0; 16];
        if text.len() != 32 {
            return Err(serde::de::Error::custom("a hash is 32 hexadecimal digits"));
        }
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use serde_json::value::RawValue;

    use super::model::{Authority, Binding, Id, Implementation, Namespace, Span};
    use super::{
        Child, ChildExport, ChildTarget, Class, Emission, Engine, Fault, Hash, Key, Level, Memo,
        Nested, Oracle, Record, Replay, Request, Root, Segment, Unit,
    };

    /// Units a test spells out; counts how often each is asked.
    #[derive(Default)]
    struct Fixture {
        units: BTreeMap<Unit, Vec<Emission>>,
        modules: BTreeMap<Hash, Vec<Child>>,
        asked: BTreeMap<Unit, usize>,
    }

    impl Oracle for Fixture {
        fn unit(&mut self, unit: &Unit) -> Result<Rc<[Emission]>, Fault> {
            *self.asked.entry(unit.clone()).or_default() += 1;
            Ok(Rc::from(self.units.get(unit).cloned().unwrap_or_default()))
        }

        fn module(&mut self, module: Hash) -> Result<Rc<[Child]>, Fault> {
            Ok(Rc::from(
                self.modules.get(&module).cloned().unwrap_or_default(),
            ))
        }

        fn segment(&mut self, _: &str) -> Result<Option<Segment>, Fault> {
            Ok(None)
        }

        fn index(&mut self) -> Result<Rc<[Box<RawValue>]>, Fault> {
            Ok(Rc::from(Vec::new()))
        }
    }

    fn hash(byte: u8) -> Hash {
        [byte; 16]
    }

    fn key(byte: u8) -> Key {
        Key {
            hash: hash(byte),
            namespace: Namespace::Type,
        }
    }

    fn id(byte: u8) -> Id {
        Id {
            authority: Authority::Compiler {
                crate_name: "fixture".to_owned(),
                stable_crate_id: "0123456789abcdef".to_owned(),
            },
            def_path_hash: super::hex(&hash(byte)),
            namespace: Namespace::Type,
        }
    }

    fn raw(text: &str) -> Box<RawValue> {
        RawValue::from_string(text.to_owned()).unwrap()
    }

    fn request(byte: u8, level: Level) -> Emission {
        Emission::Request {
            request: Request {
                key: key(byte),
                owner: raw(r#"{"kind":"free"}"#),
                level,
            },
            alias: None,
        }
    }

    fn definition(byte: u8) -> Emission {
        Emission::Definition {
            key: key(byte),
            id: id(byte),
            value: raw(&format!(r#"{{"name":{byte}}}"#)),
            alias: None,
        }
    }

    fn span() -> Span {
        Span {
            file: "lib.rs".to_owned(),
            byte_start: 0,
            byte_end: 1,
            line_start: 1,
            column_start: 1,
            line_end: 1,
            column_end: 2,
        }
    }

    /// The root module exports one definition, `1`, which the walk selects.
    fn exporting(fixture: &mut Fixture) {
        fixture.modules.insert(
            hash(0),
            vec![Child {
                name: "One".to_owned(),
                landing: None,
                export: Some(ChildExport {
                    binding: Binding::Nameable,
                    variant: None,
                    span: span(),
                    reexports: Vec::new(),
                    hops: Vec::new(),
                    target: ChildTarget::Definition {
                        key: key(1),
                        id: id(1),
                        error: None,
                        display_path: vec!["One".to_owned()],
                        request: Box::new(Ok((
                            Request {
                                key: key(1),
                                owner: raw(r#"{"kind":"free"}"#),
                                level: Level::Selected,
                            },
                            None,
                        ))),
                        module: None::<Nested>,
                    },
                }),
            }],
        );
    }

    fn root() -> Root {
        Root {
            module: hash(0),
            krate: "0123456789abcdef".to_owned(),
        }
    }

    #[test]
    fn a_shape_only_definition_upgraded_to_a_root_expands_once() {
        let mut fixture = Fixture::default();
        exporting(&mut fixture);
        // `1` asks for `2`'s shape, `2` for `3`'s, and `3` selects `2`
        // after `2` completed as a shape.
        fixture.units.insert(
            Unit::Shape(key(1)),
            vec![definition(1), request(2, Level::Shape)],
        );
        fixture.units.insert(
            Unit::Shape(key(2)),
            vec![
                definition(2),
                request(3, Level::Shape),
                request(1, Level::Shape),
            ],
        );
        fixture.units.insert(
            Unit::Shape(key(3)),
            vec![definition(3), request(2, Level::Selected)],
        );
        let extracted = Engine::new(
            &mut fixture,
            root(),
            vec!["fixture".to_owned()],
            String::new(),
        )
        .run(&[])
        .unwrap();
        assert_eq!(extracted.definitions.len(), 3);
        assert_eq!(extracted.reachable.len(), 1);
        for unit in [
            Unit::Shape(key(1)),
            Unit::Shape(key(2)),
            Unit::Shape(key(3)),
            Unit::Implementations(hash(2)),
            Unit::Surface(hash(2)),
        ] {
            assert_eq!(fixture.asked.get(&unit), Some(&1), "{unit:?}");
        }
        assert!(!fixture.asked.contains_key(&Unit::Surface(hash(3))));
    }

    #[test]
    fn an_obligation_runs_only_where_selection_gives_it_work() {
        let mut fixture = Fixture::default();
        exporting(&mut fixture);
        fixture.units.insert(
            Unit::Shape(key(1)),
            vec![
                definition(1),
                Emission::Obligation {
                    key: "concrete".to_owned(),
                    class: Class::Concrete,
                },
                Emission::Obligation {
                    key: "inert".to_owned(),
                    class: Class::Inert,
                },
                Emission::Obligation {
                    key: "parameter".to_owned(),
                    class: Class::Parameter,
                },
                request(2, Level::Shape),
            ],
        );
        fixture.units.insert(
            Unit::Shape(key(2)),
            vec![
                definition(2),
                Emission::Obligation {
                    key: "unselected".to_owned(),
                    class: Class::Parameter,
                },
            ],
        );
        Engine::new(
            &mut fixture,
            root(),
            vec!["fixture".to_owned()],
            String::new(),
        )
        .run(&[])
        .unwrap();
        let asked = |key: &str| {
            fixture.asked.contains_key(&Unit::Obligation {
                key: key.to_owned(),
            })
        };
        assert!(asked("concrete"));
        assert!(asked("parameter"), "a selected root's parameter has work");
        assert!(!asked("inert"));
        assert!(!asked("unselected"), "a shape's parameter has none");
    }

    #[test]
    fn an_implementation_gathers_every_request_into_one_record() {
        let mut fixture = Fixture::default();
        exporting(&mut fixture);
        let implementation = hash(9);
        fixture.units.insert(
            Unit::Shape(key(1)),
            vec![
                definition(1),
                Emission::Implementation {
                    implementation,
                    owner: hash(1),
                    associated: Some("member".to_owned()),
                },
                Emission::Implementation {
                    implementation,
                    owner: hash(3),
                    associated: None,
                },
            ],
        );
        fixture.units.insert(
            Unit::Base(implementation),
            vec![Emission::Base {
                implementation: Some(Box::new(Implementation {
                    key: super::hex(&implementation),
                    id: id(9),
                    relevance: vec![id(4)],
                    trait_ref: raw(r#"{"kind":"inherent"}"#),
                    self_type: raw("null"),
                    safety: raw(r#""safe""#),
                    polarity: raw(r#""positive""#),
                    const_impl: false,
                    default_impl: false,
                    generics: raw("null"),
                    associated: Vec::new(),
                    span: raw("null"),
                    origin: raw(r#"{"kind":"source"}"#),
                })),
            }],
        );
        fixture.units.insert(
            Unit::Owner {
                implementation,
                owner: hash(1),
            },
            vec![Emission::Relevance { ids: vec![id(1)] }],
        );
        fixture.units.insert(
            Unit::Owner {
                implementation,
                owner: hash(3),
            },
            vec![Emission::Relevance {
                ids: vec![id(3), id(1)],
            }],
        );
        fixture.units.insert(
            Unit::Associated {
                implementation,
                obligation: "member".to_owned(),
            },
            vec![
                Emission::Relevance { ids: vec![id(5)] },
                Emission::Associated { ids: vec![id(6)] },
            ],
        );
        let extracted = Engine::new(
            &mut fixture,
            root(),
            vec!["fixture".to_owned()],
            String::new(),
        )
        .run(&[])
        .unwrap();
        assert_eq!(extracted.implementations.len(), 1);
        let record = &extracted.implementations[0];
        assert_eq!(record.relevance, [id(1), id(3), id(4), id(5)]);
        assert_eq!(record.associated, [id(6)]);
        assert_eq!(fixture.asked.get(&Unit::Base(implementation)), Some(&1));
    }

    #[test]
    fn a_replay_of_recorded_units_gives_the_same_body_and_misses_what_it_lacks() {
        let mut fixture = Fixture::default();
        exporting(&mut fixture);
        fixture.units.insert(
            Unit::Shape(key(1)),
            vec![definition(1), request(2, Level::Shape)],
        );
        fixture
            .units
            .insert(Unit::Shape(key(2)), vec![definition(2)]);
        let mut memo = Memo::default();
        let live = {
            let mut record = Record {
                live: &mut fixture,
                memo: &mut memo,
            };
            Engine::new(
                &mut record,
                root(),
                vec!["fixture".to_owned()],
                String::new(),
            )
            .run(&[])
            .unwrap()
            .body()
            .unwrap()
        };
        let table = serde_json::to_vec(&memo.to_table(root())).unwrap();
        let memo = Memo::from_table(serde_json::from_slice(&table).unwrap());
        let replayed = Engine::new(
            &mut Replay(&memo),
            root(),
            vec!["fixture".to_owned()],
            String::new(),
        )
        .run(&[])
        .unwrap()
        .body()
        .unwrap();
        assert_eq!(live, replayed);
        let partial = Memo {
            modules: memo.modules.clone(),
            ..Memo::default()
        };
        let missing = Engine::new(
            &mut Replay(&partial),
            root(),
            vec!["fixture".to_owned()],
            String::new(),
        )
        .run(&[]);
        assert!(matches!(missing, Err(Fault::Missing(_))));
    }

    #[test]
    fn a_whole_crate_table_answers_a_later_named_demand() {
        let mut fixture = Fixture::default();
        exporting(&mut fixture);
        fixture.modules.get_mut(&hash(0)).unwrap()[0].landing = Some(Ok(super::Landing {
            requests: vec![(
                Request {
                    key: key(1),
                    owner: raw(r#"{"kind":"free"}"#),
                    level: Level::Selected,
                },
                None,
            )],
            next: super::Next::Nominal(hash(1)),
        }));
        fixture
            .units
            .insert(Unit::Shape(key(1)), vec![definition(1)]);
        let mut memo = Memo::default();
        Engine::new(
            &mut Record {
                live: &mut fixture,
                memo: &mut memo,
            },
            root(),
            vec!["original".to_owned()],
            String::new(),
        )
        .run(&[])
        .unwrap();
        let memo = Memo::from_table(
            serde_json::from_slice(&serde_json::to_vec(&memo.to_table(root())).unwrap()).unwrap(),
        );
        let demand = vec![vec!["renamed".to_owned(), "One".to_owned()]];
        let replayed = Engine::new(
            &mut Replay(&memo),
            root(),
            vec!["renamed".to_owned()],
            String::new(),
        )
        .run(&demand)
        .unwrap()
        .body()
        .unwrap();
        let live = Engine::new(
            &mut fixture,
            root(),
            vec!["renamed".to_owned()],
            String::new(),
        )
        .run(&demand)
        .unwrap()
        .body()
        .unwrap();
        assert_eq!(replayed, live);
        let document: serde_json::Value = serde_json::from_slice(&replayed).unwrap();
        assert_eq!(document["definitions"].as_array().unwrap().len(), 1);
    }
}
