//! The compiler APIs that changed inside the nightly window the driver
//! builds on (`cargo/toolchain/window.rs`). The build script sets one
//! `rawr_rustc_*` cfg for each change the building nightly has, by its
//! commit date, and the driver names the newer API through this module.

#[cfg(rawr_rustc_compiler_entrypoint)]
pub(crate) use rustc_driver::compiler_entrypoint;
#[cfg(not(rawr_rustc_compiler_entrypoint))]
pub(crate) use rustc_driver::run_compiler as compiler_entrypoint;

#[cfg(not(rawr_rustc_resolve_module))]
pub(crate) use rustc_middle::metadata::{ModChild, Reexport};
#[cfg(rawr_rustc_resolve_module)]
pub(crate) use rustc_middle::middle::resolve::{ModChild, Reexport};

#[cfg(not(rawr_rustc_mod_id))]
pub(crate) use rustc_span::def_id::LocalModDefId as LocalModId;
#[cfg(rawr_rustc_mod_id)]
pub(crate) use rustc_span::def_id::LocalModId;

#[cfg(rawr_rustc_attr_ir)]
pub(crate) use rustc_attr_ir::{StabilityLevel, UnstableReason};
#[cfg(not(rawr_rustc_attr_ir))]
pub(crate) use rustc_hir::{StabilityLevel, UnstableReason};

#[cfg(rawr_rustc_clause_polarity)]
pub(crate) use rustc_middle::ty::ClausePolarity;
#[cfg(not(rawr_rustc_clause_polarity))]
pub(crate) use rustc_middle::ty::PredicatePolarity as ClausePolarity;

/// A list of generic arguments: a reference to the interned list on an
/// older compiler, the list itself, which is `Copy`, on a newer one.
#[cfg(not(rawr_rustc_generic_args_value))]
pub(crate) type GenericArgsRef<'tcx> = rustc_middle::ty::GenericArgsRef<'tcx>;
#[cfg(rawr_rustc_generic_args_value)]
pub(crate) type GenericArgsRef<'tcx> = rustc_middle::ty::GenericArgs<'tcx>;

/// The lines of `rustc_metadata/src/locator.rs`'s "reading metadata from"
/// and "lib candidate" events, which the driver observes.
#[cfg(not(rawr_rustc_locator_search_paths))]
pub(crate) const LOCATOR_LINES: (u32, u32) = (598, 442);
#[cfg(all(rawr_rustc_locator_search_paths, not(rawr_rustc_locator_candidates)))]
pub(crate) const LOCATOR_LINES: (u32, u32) = (590, 439);
#[cfg(rawr_rustc_locator_candidates)]
pub(crate) const LOCATOR_LINES: (u32, u32) = (598, 447);

use rustc_middle::ty::{self, TyCtxt};
use rustc_span::def_id::DefId;
use rustc_span::Span;

/// The where-clauses `def_id` declares itself, without its parent's.
pub(crate) fn own_clauses<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
) -> &'tcx [(ty::Clause<'tcx>, Span)] {
    #[cfg(not(rawr_rustc_clause_queries))]
    {
        tcx.predicates_of(def_id).predicates
    }
    #[cfg(rawr_rustc_clause_queries)]
    {
        tcx.clauses_of(def_id).clauses
    }
}

/// A trait's explicit supertrait clauses.
pub(crate) fn explicit_super_clauses<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
) -> ty::EarlyBinder<'tcx, &'tcx [(ty::Clause<'tcx>, Span)]> {
    #[cfg(not(rawr_rustc_clause_queries))]
    {
        tcx.explicit_super_predicates_of(def_id)
    }
    #[cfg(rawr_rustc_clause_queries)]
    {
        tcx.explicit_super_clauses_of(def_id)
    }
}

/// Whether a definition is an anonymous constant, which an older compiler
/// splits into `AnonConst` and `InlineConst`.
pub(crate) fn is_anonymous_constant(kind: rustc_hir::def::DefKind) -> bool {
    use rustc_hir::def::DefKind;
    #[cfg(not(rawr_rustc_anon_inline_const))]
    {
        matches!(kind, DefKind::AnonConst | DefKind::InlineConst)
    }
    #[cfg(rawr_rustc_anon_inline_const)]
    {
        matches!(kind, DefKind::AnonConst)
    }
}

/// The pattern of a `Const` or `AssocConst` that is a value, not a
/// `type const`, which only an older compiler has.
#[cfg(not(rawr_rustc_no_type_const))]
macro_rules! value_const {
    ($kind:ident) => {
        rustc_hir::def::DefKind::$kind {
            is_type_const: false,
        }
    };
}
#[cfg(rawr_rustc_no_type_const)]
macro_rules! value_const {
    ($kind:ident) => {
        rustc_hir::def::DefKind::$kind
    };
}
pub(crate) use value_const;

/// A `type const`'s value. From `rawr_rustc_resolve_module` on, the
/// compiler has none for a constant whose value it doesn't represent
/// directly.
#[cfg(not(rawr_rustc_no_type_const))]
pub(crate) fn const_of_item<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
) -> Result<ty::EarlyBinder<'tcx, ty::Const<'tcx>>, String> {
    #[cfg(not(rawr_rustc_resolve_module))]
    {
        Ok(tcx.const_of_item(def_id))
    }
    #[cfg(rawr_rustc_resolve_module)]
    {
        tcx.const_of_item(def_id)
            .ok_or_else(|| "type constant has no compiler RHS".to_owned())
    }
}
