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
//!   variants) under its path.
//!
//! What differs by target (`arch`, `simd`, `os`) is left out, so the table
//! is the same on every host. `scripts/std-table.sh` regenerates it for
//! the pinned toolchain.

use std::collections::{BTreeMap, BTreeSet, HashSet};
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
/// `'a`, `T`, `S=std::hash::RandomState`, `A=alloc::alloc::Global?allocator_api`,
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
        if let Some(stability) = stability(tcx, parameter.def_id) {
            let feature = stability.split(' ').nth(1).unwrap_or("-");
            text.push('?');
            text.push_str(feature);
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
        self.items.insert(
            canonical.to_owned(),
            format!("{}\t{own}\t{generics}", kind_text(kind)),
        );
        match kind {
            DefKind::Trait => {
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
