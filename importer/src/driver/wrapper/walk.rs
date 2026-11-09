//! The instances of generic trampolines a crate's native code reaches
//! (D866), found from rustc's type check: each non-generic body is a root,
//! and a call of a local generic function at concrete types is followed
//! into that function's body at those types, as monomorphization would
//! follow it. A metadata-only check has no MIR of its dependencies, so a
//! marked generic reached only through a dependency's generic body, as a
//! `From` impl through `Column::push(impl Into<_>)`, is found by the
//! concrete type it converts: a type a body names, at concrete arguments,
//! instantiates each trampoline of an impl whose header names that type.
//!
//! A trampoline is a function whose body calls the island's `instance`
//! with its call's path; its instance is recorded as that path, the
//! `type_name` of the tuple of its type arguments, which is what the
//! trampoline sends, and each argument written from the crate root.

use std::collections::{BTreeMap, HashMap, HashSet};

use rustc_hir::def::{DefKind, Res};
use rustc_hir::intravisit::{self, Visitor};
use rustc_middle::hir::nested_filter;
use rustc_middle::ty::{self, Instance, Ty, TyCtxt, TypeVisitableExt as _};
use rustc_span::def_id::{DefId, LocalDefId, CRATE_DEF_ID};

use crate::compat::GenericArgsRef;
use crate::record;

/// The island's support module at the crate root, and the function each
/// generic trampoline calls in it.
const SUPPORT: &str = "__rawr_island";
const MARKER: &str = "instance";

/// The instances the crate's native code reaches, or why two can't be told
/// apart.
pub(super) fn instances(tcx: TyCtxt<'_>) -> Result<Vec<record::Instance>, String> {
    let Some(marker) = marker(tcx) else {
        return Ok(Vec::new());
    };
    let trampolines = trampolines(tcx, marker);
    if trampolines.is_empty() {
        return Ok(Vec::new());
    }
    let impls = impls(tcx, &trampolines);
    let mut walk = Walk {
        tcx,
        trampolines: &trampolines,
        impls: &impls,
        queue: Vec::new(),
        visited: HashSet::new(),
        found: Vec::new(),
    };
    for owner in tcx.hir_body_owners() {
        let def_id = owner.to_def_id();
        if tcx.typeck_root_def_id(def_id) == def_id
            && matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn)
            && !trampolines.contains_key(&owner)
            && !tcx.generics_of(def_id).requires_monomorphization(tcx)
        {
            walk.queue.push((owner, ty::List::empty()));
        }
    }
    walk.run();
    distinct(walk.found)
}

/// Refuse two instances of one trampoline whose type arguments differ but
/// whose `type_name`s are the same, which the runner couldn't tell apart.
fn distinct(found: Vec<record::Instance>) -> Result<Vec<record::Instance>, String> {
    let mut named = BTreeMap::<(&str, &str), &[String]>::new();
    for instance in &found {
        let key = (instance.path.as_str(), instance.name.as_str());
        match named.get(&key) {
            Some(types) if *types != instance.types.as_slice() => {
                return Err(format!(
                    "rawr can't tell `{}` at `{}` from `{}` apart: both are named `{}`",
                    instance.path,
                    types.join(", "),
                    instance.types.join(", "),
                    instance.name
                ));
            }
            _ => {
                named.insert(key, &instance.types);
            }
        }
    }
    Ok(found)
}

/// The island's `instance`, when the crate has one.
fn marker(tcx: TyCtxt<'_>) -> Option<DefId> {
    let child = |module: LocalDefId, name: &str| {
        tcx.module_children_local(module)
            .iter()
            .find(|child| child.ident.name.as_str() == name)
            .and_then(|child| child.res.opt_def_id())
    };
    let support = child(CRATE_DEF_ID, SUPPORT)?.as_local()?;
    child(support, MARKER)
}

/// Each generic trampoline, by its function, with the path its call names.
fn trampolines(tcx: TyCtxt<'_>, marker: DefId) -> HashMap<LocalDefId, String> {
    let mut found = HashMap::new();
    for owner in tcx.hir_body_owners() {
        if !matches!(tcx.def_kind(owner), DefKind::Fn | DefKind::AssocFn) {
            continue;
        }
        let mut finder = Marked { marker, path: None };
        finder.visit_body(tcx.hir_body_owned_by(owner));
        if let Some(path) = finder.path {
            found.insert(owner, path);
        }
    }
    found
}

/// Finds a call of the island's `instance` and the path it names.
struct Marked {
    marker: DefId,
    path: Option<String>,
}

impl<'tcx> Visitor<'tcx> for Marked {
    fn visit_expr(&mut self, expr: &'tcx rustc_hir::Expr<'tcx>) {
        if let rustc_hir::ExprKind::Call(callee, arguments) = expr.kind
            && let rustc_hir::ExprKind::Path(rustc_hir::QPath::Resolved(None, path)) = callee.kind
            && path.res.opt_def_id() == Some(self.marker)
            && let Some(first) = arguments.first()
            && let rustc_hir::ExprKind::Lit(literal) = first.kind
            && let rustc_ast::LitKind::Str(text, _) = literal.node
        {
            self.path = Some(text.to_string());
        }
        intravisit::walk_expr(self, expr);
    }
}

/// An impl that holds trampolines: the types its header names, and each
/// of its trampolines that has no type parameter of its own.
struct Implementation<'tcx> {
    def_id: DefId,
    header: Vec<Ty<'tcx>>,
    functions: Vec<LocalDefId>,
}

fn impls<'tcx>(
    tcx: TyCtxt<'tcx>,
    trampolines: &HashMap<LocalDefId, String>,
) -> Vec<Implementation<'tcx>> {
    let mut by_impl = HashMap::<DefId, Vec<LocalDefId>>::new();
    for &function in trampolines.keys() {
        let Some(parent) = tcx.opt_parent(function.to_def_id()) else {
            continue;
        };
        if !matches!(tcx.def_kind(parent), DefKind::Impl { .. }) {
            continue;
        }
        let own = tcx.generics_of(function);
        if own
            .own_params
            .iter()
            .any(|param| matches!(param.kind, ty::GenericParamDefKind::Type { .. }))
        {
            continue;
        }
        by_impl.entry(parent).or_default().push(function);
    }
    by_impl
        .into_iter()
        .map(|(def_id, functions)| {
            let mut header = vec![tcx.type_of(def_id).instantiate_identity().skip_norm_wip()];
            if let Some(trait_ref) = tcx.impl_opt_trait_ref(def_id) {
                header.extend(
                    trait_ref
                        .instantiate_identity()
                        .skip_norm_wip()
                        .args
                        .types(),
                );
            }
            Implementation {
                def_id,
                header,
                functions,
            }
        })
        .collect()
}

struct Walk<'tcx, 'a> {
    tcx: TyCtxt<'tcx>,
    trampolines: &'a HashMap<LocalDefId, String>,
    impls: &'a [Implementation<'tcx>],
    /// Each body to read, at the arguments its generics take.
    queue: Vec<(LocalDefId, GenericArgsRef<'tcx>)>,
    visited: HashSet<(LocalDefId, GenericArgsRef<'tcx>)>,
    found: Vec<record::Instance>,
}

impl<'tcx> Walk<'tcx, '_> {
    fn run(&mut self) {
        let mut mentioned = HashSet::new();
        while let Some((owner, args)) = self.queue.pop() {
            if !self.visited.insert((owner, args)) {
                continue;
            }
            let results = self.tcx.typeck(owner);
            let mut calls = Calls {
                tcx: self.tcx,
                results,
                args,
                calls: Vec::new(),
                types: Vec::new(),
            };
            calls.visit_body(self.tcx.hir_body_owned_by(owner));
            for (def_id, call_args) in calls.calls {
                self.reach(def_id, call_args);
            }
            for ty in calls.types {
                for part in ty.walk() {
                    if let ty::GenericArgKind::Type(inner) = part.kind()
                        && matches!(inner.kind(), ty::Adt(adt, _) if adt.did().is_local())
                        && !inner.has_param()
                        && mentioned.insert(inner)
                    {
                        self.mention(inner);
                    }
                }
            }
        }
    }

    /// A call or reference of `def_id` at concrete `args`.
    fn reach(&mut self, def_id: DefId, args: GenericArgsRef<'tcx>) {
        // A path type check left without its arguments, as a trait's
        // function named as a value, names no instance.
        if args.has_param() || args.len() != self.tcx.generics_of(def_id).count() {
            return;
        }
        let typing_env = ty::TypingEnv::fully_monomorphized();
        let Ok(Some(instance)) = Instance::try_resolve(self.tcx, typing_env, def_id, args) else {
            return;
        };
        let Some(local) = instance.def_id().as_local() else {
            return;
        };
        if self.tcx.hir_maybe_body_owned_by(local).is_some() {
            self.queue.push((local, instance.args));
        }
        if self.trampolines.contains_key(&local) {
            self.record(local, instance.args);
        }
    }

    /// A concrete local type a body names: each trampoline of an impl
    /// whose header names it, at the arguments the type gives the impl.
    fn mention(&mut self, mentioned: Ty<'tcx>) {
        let ty::Adt(adt, given) = mentioned.kind() else {
            return;
        };
        for implementation in self.impls {
            let generics = self.tcx.generics_of(implementation.def_id);
            let mut bound = vec![None; generics.count()];
            for header in &implementation.header {
                for part in header.walk() {
                    if let ty::GenericArgKind::Type(pattern) = part.kind()
                        && let ty::Adt(pattern_adt, pattern_args) = pattern.kind()
                        && pattern_adt.did() == adt.did()
                    {
                        for (pattern, given) in pattern_args.types().zip(given.types()) {
                            if let ty::Param(param) = pattern.kind() {
                                bound[param.index as usize] = Some(given);
                            }
                        }
                    }
                }
            }
            let args = generics_args(self.tcx, implementation.def_id, &bound);
            let Some(args) = args else {
                continue;
            };
            for &function in &implementation.functions {
                let own =
                    ty::GenericArgs::for_item(
                        self.tcx,
                        function.to_def_id(),
                        |param, _| match args.get(param.index as usize) {
                            Some(arg) => *arg,
                            None => self.tcx.lifetimes.re_erased.into(),
                        },
                    );
                self.record(function, own);
            }
        }
    }

    fn record(&mut self, function: LocalDefId, args: GenericArgsRef<'tcx>) {
        let tcx = self.tcx;
        let generics = tcx.generics_of(function);
        let types = (0..args.len())
            .filter(|&index| {
                matches!(
                    generics.param_at(index, tcx).kind,
                    ty::GenericParamDefKind::Type {
                        synthetic: false,
                        ..
                    }
                )
            })
            .map(|index| args.type_at(index))
            .collect::<Vec<_>>();
        let tuple = Ty::new_tup(tcx, &types);
        let written = types
            .iter()
            .map(|ty| {
                ty::print::with_crate_prefix!(ty::print::with_no_trimmed_paths!(ty.to_string()))
            })
            .collect();
        self.found.push(record::Instance {
            path: self.trampolines[&function].clone(),
            name: rustc_const_eval::util::type_name(tcx, tuple),
            types: written,
        });
    }
}

/// The arguments of `def_id`'s generics: each type `bound` gives, each
/// lifetime erased; `None` when a type or constant is left open.
fn generics_args<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    bound: &[Option<Ty<'tcx>>],
) -> Option<GenericArgsRef<'tcx>> {
    let mut open = false;
    let args = ty::GenericArgs::for_item(tcx, def_id, |param, _| match param.kind {
        ty::GenericParamDefKind::Lifetime => tcx.lifetimes.re_erased.into(),
        ty::GenericParamDefKind::Type { .. } => match bound.get(param.index as usize) {
            Some(Some(ty)) => (*ty).into(),
            _ => {
                open = true;
                tcx.types.unit.into()
            }
        },
        ty::GenericParamDefKind::Const { .. } => {
            open = true;
            tcx.types.unit.into()
        }
    });
    (!open).then_some(args)
}

/// The calls and the types of one body, at the arguments its generics
/// take.
struct Calls<'tcx> {
    tcx: TyCtxt<'tcx>,
    results: &'tcx ty::TypeckResults<'tcx>,
    args: GenericArgsRef<'tcx>,
    calls: Vec<(DefId, GenericArgsRef<'tcx>)>,
    types: Vec<Ty<'tcx>>,
}

impl<'tcx> Calls<'tcx> {
    /// `value` at the body's arguments, normalized; `None` when it can't
    /// be.
    fn concrete<T: ty::TypeFoldable<TyCtxt<'tcx>>>(&self, value: T) -> Option<T> {
        self.tcx
            .try_normalize_erasing_regions(
                ty::TypingEnv::fully_monomorphized(),
                ty::EarlyBinder::bind(self.tcx, value).instantiate(self.tcx, self.args),
            )
            .ok()
    }
}

impl<'tcx> Visitor<'tcx> for Calls<'tcx> {
    type NestedFilter = nested_filter::OnlyBodies;

    fn maybe_tcx(&mut self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn visit_expr(&mut self, expr: &'tcx rustc_hir::Expr<'tcx>) {
        let callee = match expr.kind {
            rustc_hir::ExprKind::Path(ref qpath) => {
                match self.results.qpath_res(qpath, expr.hir_id) {
                    Res::Def(DefKind::Fn | DefKind::AssocFn, def_id) => Some(def_id),
                    _ => None,
                }
            }
            rustc_hir::ExprKind::MethodCall(..) => self.results.type_dependent_def_id(expr.hir_id),
            _ => None,
        };
        if let Some(def_id) = callee
            && let Some(args) = self.concrete(self.results.node_args(expr.hir_id))
        {
            self.calls.push((def_id, args));
        }
        if let Some(ty) = self
            .results
            .node_type_opt(expr.hir_id)
            .and_then(|ty| self.concrete(ty))
        {
            self.types.push(ty);
        }
        intravisit::walk_expr(self, expr);
    }
}
