//! The crates a selected crate's table may name (D608, #672, #1097).
//!
//! A table is kept under its crate's key, which covers the crate's own
//! dependency closure and nothing else. The session loads every crate the
//! program selected, so rustc's queries for a trait's implementations, or
//! for those relevant to a type, also answer with other selected crates'
//! implementations: `iced_widget`'s for `image`'s types, which `image` never
//! reaches. A unit names an implementation only as a [`Reached`], and only
//! a [`Scope`] makes one, from a crate of the selected crate's closure, the
//! sysroot or the probe's own. So a table holds only what its key covers,
//! whatever else the program selected.

use std::collections::BTreeSet;

use rustc_middle::ty::{Ty, TyCtxt};
use rustc_span::def_id::{CrateNum, DefId, LOCAL_CRATE};

use super::{canonical_path, Budget};

/// The crates one selected crate reaches.
pub(super) struct Scope {
    crates: BTreeSet<CrateNum>,
}

/// An implementation in a selected crate's [`Scope`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct Reached(DefId);

impl Reached {
    pub(super) const fn id(self) -> DefId {
        self.0
    }
}

impl Scope {
    /// `selected`'s scope: the crates whose loaded artifacts are in its
    /// `closure` (canonical paths), the sysroot's, the probe's own and its
    /// own.
    pub(super) fn new(
        tcx: TyCtxt<'_>,
        selected: CrateNum,
        closure: &[String],
        budget: &mut Budget,
    ) -> Result<Self, String> {
        let closure = closure.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let sysroot = tcx.sess.opts.sysroot.path();
        let mut crates = BTreeSet::from([LOCAL_CRATE, selected]);
        for loaded in tcx.crates(()) {
            let source = tcx.used_crate_source(*loaded);
            let mut paths = 0_usize;
            let mut all_sysroot = true;
            let mut reached = false;
            for path in source.paths() {
                budget.edge(1)?;
                paths = paths.saturating_add(1);
                all_sysroot &= path.starts_with(sysroot);
                let path = canonical_path(path, "loaded artifact", budget)?;
                reached |= path.to_str().is_some_and(|path| closure.contains(path));
            }
            if reached || (paths != 0 && all_sysroot) {
                crates.insert(*loaded);
            }
        }
        Ok(Self { crates })
    }

    pub(super) fn contains(&self, krate: CrateNum) -> bool {
        self.crates.contains(&krate)
    }

    /// The crates in scope, the probe's own among them.
    pub(super) fn crates(&self) -> impl Iterator<Item = CrateNum> + '_ {
        self.crates.iter().copied()
    }

    /// The traits rustc can name here whose crate is in scope.
    pub(super) fn traits(&self, tcx: TyCtxt<'_>) -> Vec<DefId> {
        tcx.visible_traits()
            .filter(|trait_id| self.contains(trait_id.krate))
            .collect()
    }

    /// `implementation`, when its crate is in scope.
    pub(super) fn reached(&self, implementation: DefId) -> Option<Reached> {
        self.contains(implementation.krate)
            .then_some(Reached(implementation))
    }

    /// Every implementation of `trait_id` in scope.
    pub(super) fn implementations(&self, tcx: TyCtxt<'_>, trait_id: DefId) -> Vec<Reached> {
        tcx.all_impls(trait_id)
            .filter_map(|implementation| self.reached(implementation))
            .collect()
    }

    /// The implementations of `trait_id` in scope that may apply to
    /// `self_ty`.
    pub(super) fn relevant<'tcx>(
        &self,
        tcx: TyCtxt<'tcx>,
        trait_id: DefId,
        self_ty: Ty<'tcx>,
    ) -> Vec<Reached> {
        let mut relevant = Vec::new();
        tcx.for_each_relevant_impl(trait_id, self_ty, |implementation| {
            relevant.extend(self.reached(implementation));
        });
        relevant
    }
}
