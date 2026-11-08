//! D711 step 0: a whole walk of the sysroot's `core`, `alloc` and `std`
//! through the driver's own shapes, with each item's stability, for the
//! falsifying measurements. A prototype behind its own driver mode; nothing
//! in the resolver reads it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::{DefKind, Res};
use rustc_middle::ty::fast_reject::SimplifiedType;
use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::{CrateNum, DefId};
use rustc_span::symbol::kw;
use serde::Serialize;
use serde_json::value::RawValue;

use super::engine::Emission;
use super::{
    def_kind_text, definition_namespace, Budget, Builder, Id, Invocation, Namespace, Shape,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Stability {
    Stable,
    Unstable,
    None,
}

#[derive(Serialize)]
struct Item {
    id: Id,
    path: Vec<String>,
    kind: &'static str,
    /// The item's own attribute, then what it has once its trait's or
    /// impl's is inherited.
    stability: Stability,
    effective: Stability,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner: Option<Id>,
    /// Whether a trait's associated item has a default.
    #[serde(skip_serializing_if = "Option::is_none")]
    provided: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<Shape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct Implementation {
    id: Id,
    stability: Stability,
    effective: Stability,
    /// The trait's display path, `None` for an inherent impl.
    trait_path: Option<String>,
    /// rustc's own rendering of the self type, for lookups by eye.
    self_ty: String,
    items: Vec<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct NamePath {
    path: String,
    kind: &'static str,
    /// Every module on the way and the item are stable.
    stable: bool,
    target: Option<Id>,
}

#[derive(Serialize, Default)]
struct Timing {
    walk_ms: u128,
    shapes_ms: u128,
    implementations_ms: u128,
    write_ms: u128,
}

#[derive(Serialize)]
struct Catalog {
    target: String,
    items: Vec<Item>,
    implementations: Vec<Implementation>,
    paths: Vec<NamePath>,
    timing: Timing,
}

pub(crate) struct StdCatalog {
    pub(crate) invocation: Option<Invocation>,
    pub(crate) out: PathBuf,
    pub(crate) result: Option<Result<(), String>>,
}

impl Callbacks for StdCatalog {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        let invocation = self.invocation.take().expect("one analysis");
        self.result = Some(catalog(tcx, &invocation, &self.out));
        Compilation::Stop
    }
}

fn stability(tcx: TyCtxt<'_>, def_id: DefId) -> Stability {
    match tcx.lookup_stability(def_id) {
        Some(stability) if stability.level.is_stable() => Stability::Stable,
        Some(_) => Stability::Unstable,
        None => Stability::None,
    }
}

struct Walk<'tcx, 'a, 'b, 'c> {
    tcx: TyCtxt<'tcx>,
    builder: Builder<'tcx, 'a, 'b, 'c>,
    items: Vec<Item>,
    seen: HashSet<DefId>,
    pending: Vec<(DefId, Option<(DefId, Stability)>)>,
    adts: Vec<DefId>,
    impls: Vec<DefId>,
    impl_seen: HashSet<DefId>,
    item_stability: HashMap<DefId, Stability>,
}

fn catalog(tcx: TyCtxt<'_>, invocation: &Invocation, out: &Path) -> Result<(), String> {
    let mut budget = Budget::standard();
    let mut diagnostic = None;
    let named = |name: &str| {
        tcx.crates(())
            .iter()
            .copied()
            .find(|krate| tcx.crate_name(*krate).as_str() == name)
            .ok_or_else(|| format!("the sysroot has no `{name}`"))
    };
    let roots: Vec<(CrateNum, &str)> = vec![
        (named("core")?, "core"),
        (named("alloc")?, "alloc"),
        (named("std")?, "std"),
    ];
    let builder = Builder::new(tcx, invocation, roots[2].0, &mut budget, &mut diagnostic);
    let mut walk = Walk {
        tcx,
        builder,
        items: Vec::new(),
        seen: HashSet::new(),
        pending: Vec::new(),
        adts: Vec::new(),
        impls: Vec::new(),
        impl_seen: HashSet::new(),
        item_stability: HashMap::new(),
    };
    let mut timing = Timing::default();

    // Every public path of each crate, module by module.
    let started = Instant::now();
    let mut paths = Vec::new();
    for (krate, name) in &roots {
        let root = krate.as_def_id();
        let mut walked = HashSet::from([root]);
        let mut modules = vec![(root, name.to_string(), true)];
        walk.queue(root, None);
        while let Some((module, prefix, stable)) = modules.pop() {
            for child in tcx.module_children(module) {
                if !child.vis.is_public() || child.ident.name == kw::Underscore {
                    continue;
                }
                let path = format!("{prefix}::{}", child.ident.name);
                let Res::Def(kind, def_id) = child.res else {
                    paths.push(NamePath {
                        path,
                        kind: "primitive",
                        stable,
                        target: None,
                    });
                    continue;
                };
                let own = stability(tcx, def_id);
                let path_stable = stable && own == Stability::Stable;
                let foreign = def_id.is_crate_root();
                let target = walk.builder.id(def_id, namespace_of(kind))?;
                paths.push(NamePath {
                    path: path.clone(),
                    kind: def_kind_text(kind),
                    stable: path_stable,
                    target: Some(target),
                });
                if foreign {
                    continue;
                }
                if kind == DefKind::Mod && walked.insert(def_id) {
                    modules.push((def_id, path, path_stable));
                }
                if !matches!(kind, DefKind::Ctor(..) | DefKind::Variant | DefKind::Field) {
                    walk.queue(def_id, None);
                }
            }
        }
    }
    // The impls of each sysroot crate, and every primitive's inherent impls.
    for (krate, _) in &roots {
        for implementation in tcx.trait_impls_in_crate(*krate) {
            walk.impl_once(*implementation);
        }
    }
    for simplified in primitives(tcx) {
        for implementation in tcx.incoherent_impls(simplified) {
            walk.impl_once(*implementation);
        }
    }
    timing.walk_ms = started.elapsed().as_millis();

    let started = Instant::now();
    walk.drain()?;
    timing.shapes_ms = started.elapsed().as_millis();

    let started = Instant::now();
    let mut implementations = Vec::new();
    let mut index = 0;
    while index < walk.impls.len() {
        let implementation = walk.impls[index];
        index += 1;
        implementations.push(walk.implementation(implementation)?);
        walk.drain()?;
    }
    timing.implementations_ms = started.elapsed().as_millis();

    let started = Instant::now();
    let catalog = Catalog {
        target: invocation.selected_target.clone(),
        items: std::mem::take(&mut walk.items),
        implementations,
        paths,
        timing: Timing::default(),
    };
    let text = serde_json::to_vec(&catalog)
        .map_err(|error| format!("failed to encode the std catalog: {error}"))?;
    fs::write(out.join("catalog.json"), &text)
        .map_err(|error| format!("failed to write the std catalog: {error}"))?;
    timing.write_ms = started.elapsed().as_millis();
    let text = serde_json::to_vec_pretty(&timing).map_err(|error| error.to_string())?;
    fs::write(out.join("timing.json"), text).map_err(|error| error.to_string())?;
    Ok(())
}

fn namespace_of(kind: DefKind) -> Namespace {
    definition_namespace(kind)
}

fn primitives(tcx: TyCtxt<'_>) -> Vec<SimplifiedType> {
    use rustc_middle::ty::{FloatTy, IntTy, UintTy};
    let mut list = vec![
        SimplifiedType::Bool,
        SimplifiedType::Char,
        SimplifiedType::Str,
        SimplifiedType::Array,
        SimplifiedType::Slice,
        SimplifiedType::Never,
        SimplifiedType::Ref(rustc_hir::Mutability::Not),
        SimplifiedType::Ref(rustc_hir::Mutability::Mut),
        SimplifiedType::Ptr(rustc_hir::Mutability::Not),
        SimplifiedType::Ptr(rustc_hir::Mutability::Mut),
    ];
    for ty in [
        IntTy::Isize,
        IntTy::I8,
        IntTy::I16,
        IntTy::I32,
        IntTy::I64,
        IntTy::I128,
    ] {
        list.push(SimplifiedType::Int(ty));
    }
    for ty in [
        UintTy::Usize,
        UintTy::U8,
        UintTy::U16,
        UintTy::U32,
        UintTy::U64,
        UintTy::U128,
    ] {
        list.push(SimplifiedType::Uint(ty));
    }
    for ty in [FloatTy::F16, FloatTy::F32, FloatTy::F64, FloatTy::F128] {
        list.push(SimplifiedType::Float(ty));
    }
    for arity in 0..=12 {
        list.push(SimplifiedType::Tuple(arity));
    }
    // `impl dyn Any` and `impl dyn Error`.
    for name in ["Any", "Error"] {
        if let Some(def_id) = tcx.get_diagnostic_item(rustc_span::Symbol::intern(name)) {
            list.push(SimplifiedType::Trait(def_id));
        }
    }
    list
}

impl<'tcx> Walk<'tcx, '_, '_, '_> {
    fn queue(&mut self, def_id: DefId, owner: Option<(DefId, Stability)>) {
        if self.seen.insert(def_id) {
            self.pending.push((def_id, owner));
        }
    }

    fn impl_once(&mut self, implementation: DefId) {
        if self.impl_seen.insert(implementation) {
            self.impls.push(implementation);
        }
    }

    fn drain(&mut self) -> Result<(), String> {
        while let Some((def_id, owner)) = self.pending.pop() {
            self.item(def_id, owner)?;
        }
        Ok(())
    }

    fn item(&mut self, def_id: DefId, owner: Option<(DefId, Stability)>) -> Result<(), String> {
        let tcx = self.tcx;
        let kind = tcx.def_kind(def_id);
        let own = stability(tcx, def_id);
        let effective = match (own, owner) {
            (Stability::None, Some((_, inherited))) => inherited,
            (own, _) => own,
        };
        self.item_stability.insert(def_id, effective);
        let (shape, error) = match self.builder.local_shape(def_id) {
            Ok(shape) => (Some(shape), None),
            Err(error) => (None, Some(error)),
        };
        self.builder.emissions.clear();
        let provided = matches!(owner, Some((owner, _)) if tcx.def_kind(owner) == DefKind::Trait)
            .then(|| tcx.defaultness(def_id).has_value());
        let item = Item {
            id: self.builder.id(def_id, namespace_of(kind))?,
            path: self.builder.definition_path(def_id)?,
            kind: def_kind_text(kind),
            stability: own,
            effective,
            owner: owner
                .map(|(owner, _)| self.builder.id(owner, Namespace::Type))
                .transpose()?,
            provided,
            shape,
            error,
        };
        self.items.push(item);
        match kind {
            DefKind::Trait => {
                for member in tcx.associated_item_def_ids(def_id) {
                    self.queue(*member, Some((def_id, effective)));
                }
            }
            DefKind::Struct | DefKind::Enum | DefKind::Union => {
                self.adts.push(def_id);
                for implementation in tcx.inherent_impls(def_id) {
                    self.impl_once(*implementation);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn implementation(&mut self, implementation: DefId) -> Result<Implementation, String> {
        let tcx = self.tcx;
        let own = stability(tcx, implementation);
        let self_ty = tcx
            .type_of(implementation)
            .instantiate_identity()
            .skip_norm_wip();
        let trait_ref = tcx
            .impl_opt_trait_ref(implementation)
            .map(|reference| reference.instantiate_identity().skip_norm_wip());
        // Usable on stable when its trait and every type its self type names
        // are, and it is not itself marked unstable.
        let mut usable = own != Stability::Unstable;
        if let Some(reference) = trait_ref {
            usable &= stability(tcx, reference.def_id) == Stability::Stable;
        }
        for argument in self_ty.walk() {
            if let Some(inner) = argument.as_type()
                && let ty::Adt(definition, _) = inner.kind()
            {
                usable &= stability(tcx, definition.did()) == Stability::Stable;
            }
        }
        let effective = if usable {
            Stability::Stable
        } else {
            Stability::Unstable
        };
        let (record, error) = match self.builder.implementation_base(implementation) {
            Ok(()) => {
                let record =
                    self.builder
                        .emissions
                        .iter()
                        .rev()
                        .find_map(|emission| match emission {
                            Emission::Base {
                                implementation: Some(record),
                            } => Some(super::units::raw(record)),
                            _ => None,
                        });
                match record {
                    Some(Ok(record)) => (Some(record), None),
                    Some(Err(error)) => (None, Some(error)),
                    None => (None, Some("reservation impl".to_owned())),
                }
            }
            Err(error) => (None, Some(error)),
        };
        self.builder.emissions.clear();
        let mut items = Vec::new();
        for member in tcx.associated_item_def_ids(implementation) {
            // An inherent impl's private helpers are no program's to name.
            if trait_ref.is_none() && !tcx.visibility(*member).is_public() {
                continue;
            }
            items.push(
                self.builder
                    .id(*member, namespace_of(tcx.def_kind(*member)))?,
            );
            self.queue(*member, Some((implementation, effective)));
        }
        Ok(Implementation {
            id: self.builder.id(implementation, Namespace::Type)?,
            stability: own,
            effective,
            trait_path: trait_ref.map(|reference| {
                rustc_middle::ty::print::with_no_trimmed_paths!(tcx.def_path_str(reference.def_id))
            }),
            self_ty: rustc_middle::ty::print::with_no_trimmed_paths!(self_ty.to_string()),
            items,
            record,
            error,
        })
    }
}

/// `--std-catalog-pack`: the shipped form. Ids become indices into one
/// table, spans and origins go, and each object becomes an array of its
/// values (as a typed MessagePack encoding writes a struct). One blob for
/// every item and one for the stable subset, each with an index of byte
/// ranges by path and by trait.
pub(crate) fn pack(out: &Path) -> Result<(), String> {
    use serde_json::Value;
    let started = Instant::now();
    let text = fs::read(out.join("catalog.json")).map_err(|error| error.to_string())?;
    let catalog: Value = serde_json::from_slice(&text).map_err(|error| error.to_string())?;
    let mut ids = BTreeMap::<String, u32>::new();
    fn intern(ids: &mut BTreeMap<String, u32>, hash: &str) -> u32 {
        let next = ids.len() as u32;
        *ids.entry(hash.to_owned()).or_insert(next)
    }
    fn compact(value: &Value, ids: &mut BTreeMap<String, u32>) -> Value {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(hash)) = map.get("def_path_hash") {
                    return Value::from(intern(ids, hash));
                }
                let mut values = Vec::new();
                if let Some(kind) = map.get("kind") {
                    values.push(kind.clone());
                }
                for (key, value) in map {
                    if matches!(
                        key.as_str(),
                        "kind" | "span" | "origin" | "definition_site" | "call_site"
                    ) {
                        continue;
                    }
                    values.push(compact(value, ids));
                }
                Value::Array(values)
            }
            Value::Array(list) => {
                Value::Array(list.iter().map(|value| compact(value, ids)).collect())
            }
            other => other.clone(),
        }
    }
    let encode = |value: &Value| rmp_serde::to_vec(value).map_err(|error| error.to_string());
    let hash_of = |value: &Value| {
        value["id"]["def_path_hash"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let mut report = serde_json::Map::new();
    let inherent_owner: HashMap<String, String> = catalog["implementations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|implementation| implementation["trait_path"].is_null())
        .map(|implementation| {
            (
                hash_of(implementation),
                implementation["self_ty"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    // `portable` also drops what differs by target: `arch`, `simd`, `os`.
    for (name, stable_only, portable) in [
        ("all", false, false),
        ("stable", true, false),
        ("portable", true, true),
    ] {
        let mut blob = Vec::new();
        // path -> (offset, length); trait path -> impl ranges
        let mut by_path: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        let mut by_hash: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        let mut by_trait: BTreeMap<String, Vec<(String, u32, u32)>> = BTreeMap::new();
        let mut counts = BTreeMap::<String, u64>::new();
        let mut kept = HashSet::new();
        for item in catalog["items"].as_array().into_iter().flatten() {
            if stable_only && item["effective"] != "stable"
                || portable && target_specific(&item_path(item))
            {
                continue;
            }
            let hash = hash_of(item);
            kept.insert(hash.clone());
            let record = compact(&item_record(item), &mut ids);
            let bytes = encode(&record)?;
            let range = (blob.len() as u32, bytes.len() as u32);
            blob.extend_from_slice(&bytes);
            let path = item["path"]
                .as_array()
                .map(|segments| {
                    segments
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("::")
                })
                .unwrap_or_default();
            // An inherent method under its self type, as `i64::pow`.
            if let Some(self_ty) = item["owner"]["def_path_hash"]
                .as_str()
                .and_then(|owner| inherent_owner.get(owner))
                && let Some(name) = path.rsplit("::").next()
            {
                by_path.entry(format!("{self_ty}::{name}")).or_insert(range);
            }
            by_path.entry(path).or_insert(range);
            by_hash.insert(hash, range);
            *counts
                .entry(item["kind"].as_str().unwrap_or("?").to_owned())
                .or_default() += 1;
        }
        let mut impl_count = 0_u64;
        for implementation in catalog["implementations"].as_array().into_iter().flatten() {
            if stable_only && implementation["effective"] != "stable"
                || portable
                    && (target_specific(implementation["self_ty"].as_str().unwrap_or(""))
                        || target_specific(implementation["trait_path"].as_str().unwrap_or("")))
            {
                continue;
            }
            impl_count += 1;
            let record = compact(implementation, &mut ids);
            let bytes = encode(&record)?;
            let offset = blob.len() as u32;
            blob.extend_from_slice(&bytes);
            let key = implementation["trait_path"]
                .as_str()
                .unwrap_or("<inherent>")
                .to_owned();
            by_trait.entry(key).or_default().push((
                implementation["self_ty"].as_str().unwrap_or("").to_owned(),
                offset,
                bytes.len() as u32,
            ));
        }
        let mut paths = Vec::new();
        for path in catalog["paths"].as_array().into_iter().flatten() {
            if stable_only && path["stable"] != true
                || portable && target_specific(path["path"].as_str().unwrap_or(""))
            {
                continue;
            }
            let target = path["target"]["def_path_hash"].as_str().unwrap_or("");
            if let Some(range) = by_hash.get(target) {
                paths.push((
                    path["path"].as_str().unwrap_or("").to_owned(),
                    range.0,
                    range.1,
                ));
            }
        }
        // Each item under its definition path too, so a trait's members
        // and a type's methods are found as `core::option::Option::map`.
        for (path, range) in &by_path {
            paths.push((path.clone(), range.0, range.1));
        }
        paths.sort();
        paths.dedup_by(|left, right| left.0 == right.0);
        let index = (paths, by_trait);
        let index_bytes = rmp_serde::to_vec(&index).map_err(|error| error.to_string())?;
        fs::write(out.join(format!("{name}.bin")), &blob).map_err(|error| error.to_string())?;
        fs::write(out.join(format!("{name}.index")), &index_bytes)
            .map_err(|error| error.to_string())?;
        let mut entry = serde_json::Map::new();
        entry.insert("items".into(), Value::from(kept.len()));
        entry.insert("implementations".into(), Value::from(impl_count));
        entry.insert("paths".into(), Value::from(index.0.len()));
        entry.insert("blob_bytes".into(), Value::from(blob.len()));
        entry.insert("index_bytes".into(), Value::from(index_bytes.len()));
        entry.insert(
            "kinds".into(),
            Value::Object(
                counts
                    .into_iter()
                    .map(|(key, value)| (key, Value::from(value)))
                    .collect(),
            ),
        );
        report.insert(name.into(), Value::Object(entry));
    }
    report.insert("ids".into(), Value::from(ids.len()));
    report.insert(
        "pack_ms".into(),
        Value::from(started.elapsed().as_millis() as u64),
    );
    let text = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    fs::write(out.join("pack.json"), text).map_err(|error| error.to_string())?;
    Ok(())
}

fn item_path(item: &serde_json::Value) -> String {
    item["path"]
        .as_array()
        .map(|segments| {
            segments
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join("::")
        })
        .unwrap_or_default()
}

/// A path under a module whose contents differ by target.
fn target_specific(path: &str) -> bool {
    path.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|segment| {
            matches!(
                segment,
                "arch" | "core_arch" | "std_detect" | "simd" | "std_float" | "os"
            )
        })
}

fn item_record(item: &serde_json::Value) -> serde_json::Value {
    let mut item = item.clone();
    if let Some(map) = item.as_object_mut() {
        // The path is the index's key; the record keeps its last segment.
        map.remove("stability");
    }
    item
}

/// `--std-catalog-decode`: what a warm run pays to load the index and
/// decode, lazily, the records a program names.
pub(crate) fn decode(out: &Path, blob: &str, demand: &Path) -> Result<(), String> {
    type Index = (
        Vec<(String, u32, u32)>,
        BTreeMap<String, Vec<(String, u32, u32)>>,
    );
    let demand = fs::read_to_string(demand).map_err(|error| error.to_string())?;
    let mut report = Vec::new();
    for round in 0..5 {
        let started = Instant::now();
        let bytes = fs::read(out.join(format!("{blob}.bin"))).map_err(|error| error.to_string())?;
        let index_bytes =
            fs::read(out.join(format!("{blob}.index"))).map_err(|error| error.to_string())?;
        let read = started.elapsed();
        let started = Instant::now();
        let index: Index =
            rmp_serde::from_slice(&index_bytes).map_err(|error| error.to_string())?;
        let index_time = started.elapsed();
        let started = Instant::now();
        let mut decoded = 0_usize;
        let mut missing = Vec::new();
        for line in demand
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            if let Some(rest) = line.strip_prefix("impl ") {
                // `impl Trait for Self`: each impl of the trait whose self
                // type is that one; `impl Trait`: all of them.
                let (trait_path, self_ty) = match rest.split_once(" for ") {
                    Some((trait_path, self_ty)) => (trait_path, Some(self_ty)),
                    None => (rest, None),
                };
                let Some(list) = index.1.get(trait_path) else {
                    missing.push(line.to_owned());
                    continue;
                };
                for (ty, offset, length) in list {
                    if self_ty.is_some_and(|wanted| wanted != ty) {
                        continue;
                    }
                    let slice = &bytes[*offset as usize..(*offset + *length) as usize];
                    let _value: serde_json::Value =
                        rmp_serde::from_slice(slice).map_err(|error| error.to_string())?;
                    decoded += 1;
                }
            } else {
                match index.0.binary_search_by(|entry| entry.0.as_str().cmp(line)) {
                    Ok(position) => {
                        let (_, offset, length) = &index.0[position];
                        let slice = &bytes[*offset as usize..(*offset + *length) as usize];
                        let _value: serde_json::Value =
                            rmp_serde::from_slice(slice).map_err(|error| error.to_string())?;
                        decoded += 1;
                    }
                    Err(_) => missing.push(line.to_owned()),
                }
            }
        }
        let decode_time = started.elapsed();
        report.push(format!(
            "round {round}: read {:.3} ms, index {:.3} ms ({} paths), decode {:.3} ms ({decoded} records), missing {missing:?}",
            read.as_secs_f64() * 1000.0,
            index_time.as_secs_f64() * 1000.0,
            index.0.len(),
            decode_time.as_secs_f64() * 1000.0,
        ));
    }
    for line in report {
        println!("{line}");
    }
    Ok(())
}
