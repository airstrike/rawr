//! D711 step 1: the std table rawr embeds (`src/std_table/`). One walk of
//! the sysroot's `core`, `alloc` and `std` writes two sorted, tab-separated
//! files:
//!
//! - `paths.tsv`: every public path a program can write, with its
//!   namespace (`t`, `v` or `m`), its kind, the stability of the path (its
//!   modules' and its item's: `s`, or `u feature issue reason`), and the
//!   definition it names (`=` when it is that path);
//! - `items.tsv`: each definition, with its own stability and generics,
//!   and its members (a trait's items, a type's inherent items, an enum's
//!   variants) under its path. A trait's line ends with its supertraits,
//!   and each of its members' with its declaration as a program writes it
//!   (D711 step 2), naming types by their public paths, bare for the
//!   prelude's.
//!
//! What differs by target (`arch`, `simd`, `os`) is left out, so the table
//! is the same on every host. `scripts/std-table.sh` regenerates it for
//! the pinned toolchain.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use rustc_driver::{Callbacks, Compilation};
use rustc_hir::def::{CtorOf, DefKind, Res};
use rustc_middle::ty::fast_reject::SimplifiedType;
use rustc_middle::ty::{self, GenericParamDefKind, TyCtxt};
use rustc_span::def_id::{CrateNum, DefId};
use rustc_span::symbol::kw;

pub(crate) struct StdTable {
    pub(crate) out: PathBuf,
    pub(crate) result: Option<Result<(), String>>,
}

impl Callbacks for StdTable {
    fn after_analysis<'tcx>(
        &mut self,
        _compiler: &rustc_interface::interface::Compiler,
        tcx: TyCtxt<'tcx>,
    ) -> Compilation {
        self.result = Some(table(tcx, &self.out));
        Compilation::Stop
    }
}

/// `s` for a stable item (or one with no attribute), else `u` and its
/// feature, issue and reason: `u allocator_api 32838 -`.
fn stability(tcx: TyCtxt<'_>, def_id: DefId) -> Option<String> {
    use rustc_hir::{StabilityLevel, UnstableReason};
    let stability = tcx.lookup_stability(def_id)?;
    match stability.level {
        StabilityLevel::Stable { .. } => None,
        StabilityLevel::Unstable { reason, issue, .. } => {
            let reason = match reason {
                UnstableReason::Some(reason) => clean(reason.as_str()),
                UnstableReason::None | UnstableReason::Default => "-".to_owned(),
            };
            let issue = issue.map_or_else(|| "-".to_owned(), |issue| issue.to_string());
            Some(format!("u {} {issue} {reason}", stability.feature))
        }
    }
}

/// One line's text: no tabs or newlines.
fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn kind_text(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Mod => "mod",
        DefKind::Struct => "struct",
        DefKind::Union => "union",
        DefKind::Enum => "enum",
        DefKind::Variant => "variant",
        DefKind::Ctor(CtorOf::Variant, _) => "variant",
        DefKind::Ctor(CtorOf::Struct, _) => "struct",
        DefKind::Trait => "trait",
        DefKind::TraitAlias => "trait-alias",
        DefKind::TyAlias => "type",
        DefKind::ForeignTy => "foreign-type",
        DefKind::AssocTy => "assoc-type",
        DefKind::Fn => "fn",
        DefKind::AssocFn => "assoc-fn",
        DefKind::Const { .. } => "const",
        DefKind::AssocConst { .. } => "assoc-const",
        DefKind::Static { .. } => "static",
        DefKind::Macro(_) => "macro",
        _ => "other",
    }
}

/// A path under a module whose contents differ by target.
fn target_specific(path: &str) -> bool {
    path.split("::").any(|segment| {
        matches!(
            segment,
            "arch" | "core_arch" | "std_detect" | "simd" | "std_float" | "os"
        )
    })
}

/// The definition's own path, from its crate: `alloc::vec::Vec`.
fn definition_path(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let def_id = match tcx.def_kind(def_id) {
        DefKind::Ctor(..) => tcx.parent(def_id),
        _ => def_id,
    };
    let mut segments = vec![tcx.crate_name(def_id.krate).to_string()];
    for data in tcx.def_path(def_id).data {
        match data.data.get_opt_name() {
            Some(name) => segments.push(name.to_string()),
            None => segments.push(format!("{{{}}}", data.data)),
        }
    }
    segments.join("::")
}

fn render_ty<'tcx>(ty: ty::Ty<'tcx>) -> String {
    clean(&rustc_middle::ty::print::with_no_trimmed_paths!(
        ty.to_string()
    ))
}

/// The generic parameters a path to the item may write, in order:
/// `'a`, `T`, `S=std::hash::RandomState`, `A=std::alloc::Global?allocator_api#32838`,
/// `const N`. Joined with ` ` (a default's own spaces become nothing).
fn generics(tcx: TyCtxt<'_>, def_id: DefId) -> String {
    let generics = tcx.generics_of(def_id);
    let mut out = Vec::new();
    for parameter in &generics.own_params {
        let mut text = match parameter.kind {
            GenericParamDefKind::Lifetime => {
                format!("'{}", parameter.name.as_str().trim_start_matches('\''))
            }
            GenericParamDefKind::Type {
                synthetic: true, ..
            } => continue,
            GenericParamDefKind::Type { has_default, .. } => {
                if parameter.name == kw::SelfUpper {
                    continue;
                }
                let mut text = parameter.name.to_string();
                if has_default {
                    let default = tcx
                        .type_of(parameter.def_id)
                        .instantiate_identity()
                        .skip_norm_wip();
                    text.push('=');
                    text.push_str(&render_ty(default).replace(' ', ""));
                }
                text
            }
            GenericParamDefKind::Const { has_default, .. } => {
                let mut text = format!("const:{}", parameter.name);
                if has_default {
                    text.push_str("=_");
                }
                text
            }
        };
        // An unstable parameter: `?feature`, then `#issue` when it has one.
        if let Some(stability) = stability(tcx, parameter.def_id) {
            let mut parts = stability.split(' ').skip(1);
            text.push('?');
            text.push_str(parts.next().unwrap_or("-"));
            if let Some(issue) = parts.next().filter(|issue| *issue != "-") {
                text.push('#');
                text.push_str(issue);
            }
        }
        out.push(text);
    }
    out.join(" ")
}

struct Table<'tcx> {
    tcx: TyCtxt<'tcx>,
    paths: BTreeMap<String, String>,
    items: BTreeMap<String, String>,
    seen: HashSet<DefId>,
    /// Each type's and trait's best public path, as a signature names it.
    public: HashMap<DefId, (bool, String)>,
    /// The definitions the 2024 prelude names, which a signature names bare.
    prelude: HashSet<DefId>,
    /// The stable traits, with their paths, whose members' signatures are
    /// written once every public path is known.
    traits: Vec<(DefId, String)>,
}

fn table(tcx: TyCtxt<'_>, out: &Path) -> Result<(), String> {
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
    let mut table = Table {
        tcx,
        paths: BTreeMap::new(),
        items: BTreeMap::new(),
        seen: HashSet::new(),
        public: HashMap::new(),
        prelude: HashSet::new(),
        traits: Vec::new(),
    };
    for (krate, name) in &roots {
        let root = krate.as_def_id();
        table
            .paths
            .insert(format!("{name}\tt"), "mod\ts\t=".to_owned());
        // Every path, through re-exported modules too; a module is walked
        // once per path it is reached by, never twice on one path.
        let mut modules = vec![(root, (*name).to_owned(), None::<String>, vec![root])];
        while let Some((module, prefix, unstable, stack)) = modules.pop() {
            for child in tcx.module_children(module) {
                if !child.vis.is_public() || child.ident.name == kw::Underscore {
                    continue;
                }
                let path = format!("{prefix}::{}", child.ident.name);
                if target_specific(&path) {
                    continue;
                }
                match child.res {
                    Res::PrimTy(primitive) => {
                        let stability = unstable.clone().unwrap_or_else(|| "s".to_owned());
                        table.path(path, 't', "primitive", &stability, primitive.name_str());
                    }
                    Res::Def(kind, def_id) => {
                        // A unit or tuple struct's constructor is its type's
                        // path in the value namespace: the type's line
                        // answers for both.
                        if matches!(kind, DefKind::Field | DefKind::Ctor(CtorOf::Struct, _)) {
                            continue;
                        }
                        let namespace = match kind.ns() {
                            Some(rustc_hir::def::Namespace::TypeNS) => 't',
                            Some(rustc_hir::def::Namespace::ValueNS) => 'v',
                            Some(rustc_hir::def::Namespace::MacroNS) => 'm',
                            None => continue,
                        };
                        let own = stability(tcx, def_id);
                        let stability = unstable.clone().or(own.clone());
                        let canonical = if def_id.is_crate_root() {
                            tcx.crate_name(def_id.krate).to_string()
                        } else {
                            definition_path(tcx, def_id)
                        };
                        if target_specific(&canonical) {
                            continue;
                        }
                        let stability_text = stability.clone().unwrap_or_else(|| "s".to_owned());
                        table.path(
                            path.clone(),
                            namespace,
                            kind_text(kind),
                            &stability_text,
                            &canonical,
                        );
                        if namespace == 't' && kind != DefKind::Mod {
                            table.public_path(def_id, &path, stability.is_none());
                        }
                        if kind == DefKind::Mod {
                            if !def_id.is_crate_root() && !stack.contains(&def_id) {
                                let mut stack = stack.clone();
                                stack.push(def_id);
                                modules.push((def_id, path, stability, stack));
                            }
                        } else {
                            table.item(def_id, &canonical);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // Each trait's supertraits and its members' signatures, in the public
    // paths a program writes.
    for (def_id, canonical) in std::mem::take(&mut table.traits) {
        table.trait_signatures(def_id, &canonical);
    }
    // Every primitive's inherent items, under its name: `i64::pow`.
    for (name, simplified) in primitives() {
        let mut members = BTreeSet::new();
        for implementation in tcx.incoherent_impls(simplified) {
            table.inherent_members(*implementation, name, &mut members);
        }
        table
            .items
            .insert(name.to_owned(), "primitive\ts\t".to_owned());
    }
    write_sorted(out.join("paths.tsv"), &table.paths)?;
    write_sorted(out.join("items.tsv"), &table.items)?;
    Ok(())
}

fn write_sorted(path: PathBuf, lines: &BTreeMap<String, String>) -> Result<(), String> {
    let mut text = String::new();
    for (key, value) in lines {
        let _ = writeln!(text, "{key}\t{value}");
    }
    fs::write(&path, text).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn primitives() -> Vec<(&'static str, SimplifiedType)> {
    use rustc_middle::ty::{FloatTy, IntTy, UintTy};
    let mut list = vec![
        ("bool", SimplifiedType::Bool),
        ("char", SimplifiedType::Char),
        ("str", SimplifiedType::Str),
        ("array", SimplifiedType::Array),
        ("slice", SimplifiedType::Slice),
    ];
    for (name, ty) in [
        ("isize", IntTy::Isize),
        ("i8", IntTy::I8),
        ("i16", IntTy::I16),
        ("i32", IntTy::I32),
        ("i64", IntTy::I64),
        ("i128", IntTy::I128),
    ] {
        list.push((name, SimplifiedType::Int(ty)));
    }
    for (name, ty) in [
        ("usize", UintTy::Usize),
        ("u8", UintTy::U8),
        ("u16", UintTy::U16),
        ("u32", UintTy::U32),
        ("u64", UintTy::U64),
        ("u128", UintTy::U128),
    ] {
        list.push((name, SimplifiedType::Uint(ty)));
    }
    for (name, ty) in [
        ("f16", FloatTy::F16),
        ("f32", FloatTy::F32),
        ("f64", FloatTy::F64),
        ("f128", FloatTy::F128),
    ] {
        list.push((name, SimplifiedType::Float(ty)));
    }
    list
}

impl<'tcx> Table<'tcx> {
    /// The path's line in one namespace (`t`ype, `v`alue or `m`acro).
    fn path(
        &mut self,
        path: String,
        namespace: char,
        kind: &str,
        stability: &str,
        canonical: &str,
    ) {
        let target = if path == canonical { "=" } else { canonical };
        self.paths
            .entry(format!("{path}\t{namespace}"))
            .or_insert_with(|| format!("{kind}\t{stability}\t{target}"));
    }

    /// The definition's line, and its members' under its path.
    fn item(&mut self, def_id: DefId, canonical: &str) {
        let tcx = self.tcx;
        let def_id = match tcx.def_kind(def_id) {
            DefKind::Ctor(..) => tcx.parent(def_id),
            _ => def_id,
        };
        if !self.seen.insert(def_id) {
            return;
        }
        let kind = tcx.def_kind(def_id);
        let own = stability(tcx, def_id).unwrap_or_else(|| "s".to_owned());
        let generics = match kind {
            DefKind::Struct
            | DefKind::Enum
            | DefKind::Union
            | DefKind::Trait
            | DefKind::TyAlias
            | DefKind::Fn => generics(tcx, def_id),
            _ => String::new(),
        };
        // A derive macro shares its trait's path (`core::cmp::Ord`): the
        // trait's line answers.
        if kind_text(kind) == "macro" && self.items.contains_key(canonical) {
            return;
        }
        self.items.insert(
            canonical.to_owned(),
            format!("{}\t{own}\t{generics}", kind_text(kind)),
        );
        match kind {
            DefKind::Trait => {
                self.traits.push((def_id, canonical.to_owned()));
                for member in tcx.associated_item_def_ids(def_id) {
                    let item = tcx.associated_item(*member);
                    let Some(name) = item.opt_name() else {
                        continue;
                    };
                    let member_kind = kind_text(tcx.def_kind(*member));
                    let stability = stability(tcx, *member).unwrap_or_else(|| own.clone());
                    let provided = if item.defaultness(tcx).has_value() {
                        "provided"
                    } else {
                        "required"
                    };
                    self.items.insert(
                        format!("{canonical}::{name}"),
                        format!("{member_kind}\t{stability}\t{provided}"),
                    );
                }
            }
            DefKind::Enum => {
                for variant in tcx.adt_def(def_id).variants() {
                    let stability = stability(tcx, variant.def_id).unwrap_or_else(|| own.clone());
                    self.items.insert(
                        format!("{canonical}::{}", variant.name),
                        format!("variant\t{stability}\t"),
                    );
                }
                self.inherent(def_id, canonical);
            }
            DefKind::Struct | DefKind::Union => self.inherent(def_id, canonical),
            _ => {}
        }
    }

    fn inherent(&mut self, def_id: DefId, canonical: &str) {
        let mut members = BTreeSet::new();
        for implementation in self.tcx.inherent_impls(def_id) {
            self.inherent_members(*implementation, canonical, &mut members);
        }
    }

    /// An inherent impl's public items under `owner`; the first impl to
    /// declare a name gives its line.
    fn inherent_members(
        &mut self,
        implementation: DefId,
        owner: &str,
        seen: &mut BTreeSet<String>,
    ) {
        let tcx = self.tcx;
        let inherited = stability(tcx, implementation);
        for member in tcx.associated_item_def_ids(implementation) {
            if !tcx.visibility(*member).is_public() {
                continue;
            }
            let Some(name) = tcx.associated_item(*member).opt_name() else {
                continue;
            };
            let key = format!("{owner}::{name}");
            seen.insert(key.clone());
            let stability = stability(tcx, *member)
                .or_else(|| inherited.clone())
                .unwrap_or_else(|| "s".to_owned());
            let kind = kind_text(tcx.def_kind(*member));
            // A stable method beside an unstable one of its name keeps the
            // stable line.
            match self.items.get(&key) {
                Some(line) if line.split('\t').nth(1) == Some("s") => {}
                _ => {
                    self.items.insert(key, format!("{kind}\t{stability}\t"));
                }
            }
        }
    }
}

/// Signatures in the paths a program writes (D711 step 2): a trait's
/// supertraits, its associated types' bounds and its functions'
/// declarations, which rawr reads as a program's own trait.
impl<'tcx> Table<'tcx> {
    /// Keep `path` as `def_id`'s public path when it is the best yet: a
    /// stable one, then one under `std`, then the shortest.
    fn public_path(&mut self, def_id: DefId, path: &str, stable: bool) {
        if let Some(name) = path.strip_prefix("std::prelude::rust_2024::")
            && !name.contains("::")
            && stable
        {
            self.prelude.insert(def_id);
        }
        if path.starts_with("std::prelude::")
            || path.starts_with("core::prelude::")
            || path.contains("::__")
        {
            return;
        }
        let rank = |stable: bool, path: &str| {
            (
                !stable,
                !path.starts_with("std::"),
                path.matches("::").count(),
                path.to_owned(),
            )
        };
        let better = match self.public.get(&def_id) {
            Some((kept_stable, kept)) => rank(stable, path) < rank(*kept_stable, kept),
            None => true,
        };
        if better {
            self.public.insert(def_id, (stable, path.to_owned()));
        }
    }

    /// The path a signature names `def_id` by: bare for a prelude name.
    fn name_of(&self, def_id: DefId) -> String {
        if self.prelude.contains(&def_id) {
            return self.tcx.item_name(def_id).to_string();
        }
        match self.public.get(&def_id) {
            Some((_, path)) => path.clone(),
            None => definition_path(self.tcx, def_id),
        }
    }

    /// A path's written type arguments: lifetimes left out, and the
    /// trailing ones that are their parameters' defaults.
    fn arguments(
        &self,
        def_id: DefId,
        args: ty::GenericArgsRef<'tcx>,
        skip_self: bool,
    ) -> Vec<String> {
        let generics = self.tcx.generics_of(def_id);
        // `own_args_no_defaults` leaves a trait's `Self` out already.
        let (own, skip) = if args.len() == generics.count() {
            (generics.own_args_no_defaults(self.tcx, args), 0)
        } else {
            let start = generics
                .own_params
                .first()
                .map_or(0, |first| first.index as usize);
            let end = args.len().min(start + generics.own_params.len());
            (&args[start.min(end)..end], usize::from(skip_self))
        };
        own.iter()
            .skip(skip)
            .filter_map(|arg| match arg.kind() {
                ty::GenericArgKind::Type(ty) => Some(self.ty(ty)),
                ty::GenericArgKind::Const(constant) => Some(constant.to_string()),
                ty::GenericArgKind::Lifetime(_) => None,
            })
            .collect()
    }

    fn ty(&self, ty: ty::Ty<'tcx>) -> String {
        let tcx = self.tcx;
        let list = |items: Vec<String>| {
            if items.is_empty() {
                String::new()
            } else {
                format!("<{}>", items.join(", "))
            }
        };
        match ty.kind() {
            ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Str => {
                ty.to_string()
            }
            ty::Never => "!".to_owned(),
            ty::Adt(definition, args) => format!(
                "{}{}",
                self.name_of(definition.did()),
                list(self.arguments(definition.did(), args, false))
            ),
            ty::Foreign(def_id) => self.name_of(*def_id),
            ty::Array(element, length) => format!("[{}; {length}]", self.ty(*element)),
            ty::Slice(element) => format!("[{}]", self.ty(*element)),
            ty::RawPtr(target, mutability) => format!(
                "*{} {}",
                if mutability.is_mut() { "mut" } else { "const" },
                self.ty(*target)
            ),
            ty::Ref(_, target, mutability) => format!(
                "&{}{}",
                if mutability.is_mut() { "mut " } else { "" },
                self.ty(*target)
            ),
            ty::Tuple(types) => match types.len() {
                1 => format!("({},)", self.ty(types[0])),
                _ => format!(
                    "({})",
                    types
                        .iter()
                        .map(|ty| self.ty(ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            ty::Param(parameter) => parameter.name.to_string(),
            ty::Alias(_, alias) if let ty::AliasTyKind::Projection { def_id } = alias.kind => {
                let associated: DefId = def_id;
                // A return-position `impl Trait` in a trait has no name.
                let Some(name) = tcx.opt_item_name(associated) else {
                    return "_".to_owned();
                };
                let self_ty = alias.args.type_at(0);
                if matches!(self_ty.kind(), ty::Param(parameter) if parameter.name == kw::SelfUpper)
                {
                    format!("Self::{name}")
                } else {
                    let trait_id = tcx.parent(associated);
                    format!(
                        "<{} as {}{}>::{name}",
                        self.ty(self_ty),
                        self.name_of(trait_id),
                        list(self.arguments(trait_id, alias.args, true))
                    )
                }
            }
            ty::Dynamic(predicates, _) => {
                let mut bounds = Vec::new();
                if let Some(principal) = predicates.principal() {
                    let principal = principal
                        .skip_binder()
                        .with_self_ty(tcx, tcx.types.trait_object_dummy_self);
                    let mut arguments = self.arguments(principal.def_id, principal.args, true);
                    for projection in predicates.projection_bounds() {
                        let projection = projection.skip_binder();
                        if let Some(term) = projection.term.as_type() {
                            arguments.push(format!(
                                "{} = {}",
                                tcx.opt_item_name(projection.def_id)
                                    .map_or_else(|| "_".to_owned(), |name| name.to_string()),
                                self.ty(term)
                            ));
                        }
                    }
                    bounds.push(format!(
                        "{}{}",
                        self.name_of(principal.def_id),
                        list(arguments)
                    ));
                }
                for auto in predicates.auto_traits() {
                    bounds.push(self.name_of(auto));
                }
                format!("dyn {}", bounds.join(" + "))
            }
            _ => clean(&rustc_middle::ty::print::with_no_trimmed_paths!(
                ty.to_string()
            )),
        }
    }

    /// The bounds `clauses` put on `subject`, as a program writes them:
    /// `Iterator<Item = A> + Clone`, and `?Sized` when none says `Sized`.
    fn bounds(&self, subject: ty::Ty<'tcx>, clauses: &[ty::Clause<'tcx>]) -> Vec<String> {
        let tcx = self.tcx;
        let sized = tcx.lang_items().sized_trait();
        let mut is_sized = false;
        let mut bounds: Vec<(DefId, Vec<String>, Vec<String>)> = Vec::new();
        for clause in clauses {
            match clause.kind().skip_binder() {
                ty::ClauseKind::Trait(predicate)
                    if predicate.self_ty() == subject
                        && predicate.polarity == ty::PredicatePolarity::Positive =>
                {
                    let def_id = predicate.def_id();
                    if Some(def_id) == sized {
                        is_sized = true;
                        continue;
                    }
                    // The sizedness hierarchy below `Sized` is implied, and
                    // an unstable trait (`Destruct`) no program names.
                    if Some(def_id) == tcx.lang_items().meta_sized_trait()
                        || Some(def_id) == tcx.lang_items().pointee_sized_trait()
                        || stability(tcx, def_id).is_some()
                    {
                        continue;
                    }
                    let arguments = self.arguments(def_id, predicate.trait_ref.args, true);
                    bounds.push((def_id, arguments, Vec::new()));
                }
                ty::ClauseKind::Projection(predicate)
                    if predicate.projection_term.args.type_at(0) == subject =>
                {
                    let associated: DefId = predicate.def_id();
                    let trait_id = tcx.parent(associated);
                    let Some(term) = predicate.term.as_type() else {
                        continue;
                    };
                    let Some(name) = tcx.opt_item_name(associated) else {
                        continue;
                    };
                    let text = format!("{name} = {}", self.ty(term));
                    match bounds.iter_mut().find(|(def_id, ..)| *def_id == trait_id) {
                        Some((.., projections)) => projections.push(text),
                        None => bounds.push((trait_id, Vec::new(), vec![text])),
                    }
                }
                _ => {}
            }
        }
        // `F: FnMut(A) -> B` is `F: FnMut<(A,)>` and `<F as
        // FnOnce<(A,)>>::Output == B`: one bound in its sugar.
        let closure = |def_id: DefId| tcx.fn_trait_kind_from_def_id(def_id).is_some();
        if bounds
            .iter()
            .any(|(def_id, arguments, _)| closure(*def_id) && !arguments.is_empty())
        {
            let output = bounds
                .iter()
                .filter(|(def_id, ..)| closure(*def_id))
                .flat_map(|(.., projections)| projections.clone())
                .collect::<Vec<_>>();
            bounds.retain(|(def_id, arguments, _)| !closure(*def_id) || !arguments.is_empty());
            for (def_id, _, projections) in &mut bounds {
                if closure(*def_id) {
                    *projections = output.clone();
                }
            }
        }
        let mut out = bounds
            .into_iter()
            .map(|(def_id, arguments, projections)| {
                // `FnMut(A) -> B`: a closure trait in its sugar.
                if tcx.fn_trait_kind_from_def_id(def_id).is_some() {
                    let inputs = arguments
                        .first()
                        .map(|tuple| {
                            let inner = tuple.strip_prefix('(').unwrap_or(tuple);
                            let inner = inner.strip_suffix(')').unwrap_or(inner);
                            inner.strip_suffix(',').unwrap_or(inner).to_owned()
                        })
                        .unwrap_or_default();
                    let output = projections
                        .iter()
                        .find_map(|projection| projection.strip_prefix("Output = "))
                        .filter(|output| *output != "()")
                        .map(|output| format!(" -> {output}"))
                        .unwrap_or_default();
                    return format!("{}({inputs}){output}", self.name_of(def_id));
                }
                let all = arguments.into_iter().chain(projections).collect::<Vec<_>>();
                if all.is_empty() {
                    self.name_of(def_id)
                } else {
                    format!("{}<{}>", self.name_of(def_id), all.join(", "))
                }
            })
            .collect::<Vec<_>>();
        if !is_sized {
            out.push("?Sized".to_owned());
        }
        out
    }

    /// A trait's supertraits on its line, and each member's declaration
    /// on the member's: `fn from_str(s: &str) -> Result<Self, Self::Err>`,
    /// or `type IntoIter: Iterator<Item = Self::Item>`.
    fn trait_signatures(&mut self, def_id: DefId, canonical: &str) {
        let tcx = self.tcx;
        let self_ty = tcx.types.self_param;
        let supers = tcx
            .explicit_super_predicates_of(def_id)
            .skip_binder()
            .iter()
            .map(|(clause, _)| *clause)
            .collect::<Vec<_>>();
        let mut supertraits = self.bounds(self_ty, &supers);
        // A trait's `Self` is `?Sized` unless it says `Sized`, which its
        // super predicates name.
        supertraits.retain(|bound| bound != "?Sized");
        let sized = tcx.lang_items().sized_trait();
        if supers.iter().any(|clause| {
            matches!(clause.kind().skip_binder(),
                ty::ClauseKind::Trait(predicate) if Some(predicate.def_id()) == sized)
        }) {
            supertraits.insert(0, "Sized".to_owned());
        }
        let supertraits = supertraits.join(" + ");
        if let Some(line) = self.items.get_mut(canonical) {
            line.push('\t');
            line.push_str(&supertraits);
        }
        for member in tcx.associated_item_def_ids(def_id) {
            let item = tcx.associated_item(*member);
            let Some(name) = item.opt_name() else {
                continue;
            };
            let declaration = match tcx.def_kind(*member) {
                DefKind::AssocFn => self.function(*member, name.as_str()),
                DefKind::AssocTy => {
                    let alias = ty::Ty::new_projection_from_args(
                        tcx,
                        ty::IsRigid::No,
                        *member,
                        ty::GenericArgs::identity_for_item(tcx, *member),
                    );
                    let clauses = tcx
                        .explicit_item_bounds(*member)
                        .skip_binder()
                        .iter()
                        .map(|(clause, _)| *clause)
                        .collect::<Vec<_>>();
                    let bounds = self.bounds(alias, &clauses);
                    if bounds.is_empty() {
                        format!("type {name}")
                    } else {
                        format!("type {name}: {}", bounds.join(" + "))
                    }
                }
                DefKind::AssocConst { .. } => format!(
                    "const {name}: {}",
                    self.ty(tcx.type_of(*member).instantiate_identity().skip_norm_wip())
                ),
                _ => continue,
            };
            if let Some(line) = self.items.get_mut(&format!("{canonical}::{name}")) {
                line.push('\t');
                line.push_str(&declaration);
            }
        }
    }

    /// `fn name<T: Bound>(&self, a: A) -> R`, a trait function's
    /// declaration.
    fn function(&self, def_id: DefId, name: &str) -> String {
        let tcx = self.tcx;
        let signature = tcx
            .fn_sig(def_id)
            .instantiate_identity()
            .skip_norm_wip()
            .skip_binder();
        let clauses = tcx
            .predicates_of(def_id)
            .predicates
            .iter()
            .map(|(clause, _)| *clause)
            .collect::<Vec<_>>();
        let generics = tcx.generics_of(def_id);
        let mut parameters = Vec::new();
        for parameter in &generics.own_params {
            if !matches!(parameter.kind, GenericParamDefKind::Type { .. }) {
                continue;
            }
            let ty = ty::Ty::new_param(tcx, parameter.index, parameter.name);
            let bounds = self.bounds(ty, &clauses);
            let name = self.ty(ty);
            if bounds.is_empty() {
                parameters.push(name);
            } else {
                parameters.push(format!("{name}: {}", bounds.join(" + ")));
            }
        }
        let names = tcx.fn_arg_idents(def_id);
        let method = tcx.associated_item(def_id).is_method();
        let is_self = |ty: ty::Ty<'tcx>| matches!(ty.kind(), ty::Param(parameter) if parameter.name == kw::SelfUpper);
        let mut inputs = Vec::new();
        for (index, input) in signature.inputs().iter().enumerate() {
            if method && index == 0 {
                inputs.push(match input.kind() {
                    _ if is_self(*input) => "self".to_owned(),
                    ty::Ref(_, target, mutability) if is_self(*target) => if mutability.is_mut() {
                        "&mut self"
                    } else {
                        "&self"
                    }
                    .to_owned(),
                    _ => format!("self: {}", self.ty(*input)),
                });
                continue;
            }
            let name = names
                .get(index)
                .copied()
                .flatten()
                .map(|ident| ident.name.to_string())
                .filter(|name| name != "_" && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .unwrap_or_else(|| format!("a{index}"));
            inputs.push(format!("{name}: {}", self.ty(*input)));
        }
        let mut text = format!("fn {name}");
        if !parameters.is_empty() {
            let _ = write!(text, "<{}>", parameters.join(", "));
        }
        let _ = write!(text, "({})", inputs.join(", "));
        if !signature.output().is_unit() {
            let _ = write!(text, " -> {}", self.ty(signature.output()));
        }
        text
    }
}
