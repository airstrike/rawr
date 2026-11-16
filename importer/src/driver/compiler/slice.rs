//! A selected trait's members on a slice, which no crate type holds, through
//! a blanket impl over a borrow of its own type, as `impl<'data, I: 'data +
//! ?Sized> IntoParallelRefIterator<'data> for I where &'data I:
//! IntoParallelIterator` gives `[T]` its `par_iter` through
//! `IntoParallelIterator`'s `impl<T: Sync> .. for &[T]` (`surface::slice`).
//! The classifier composes the two impls, so both are kept: the blanket
//! one with each of its trait's items, and each impl of the bound's trait
//! for `&[T]` with the associated types its member's output names.

use rustc_middle::ty;
use rustc_span::def_id::DefId;

use super::{AssociatedDemandKind, AssociatedObligation, Builder};

impl Builder<'_, '_, '_, '_> {
    /// Keeps each blanket impl of the trait `trait_id` over a parameter `I`
    /// with a bound `&I: Other` of a crate trait `Other`, and `Other`'s
    /// impls for `&[T]`, when it has any.
    pub(super) fn request_slice_applications(&mut self, trait_id: DefId) -> Result<(), String> {
        let tcx = self.tcx;
        let items = tcx
            .associated_items(trait_id)
            .in_definition_order()
            .map(|item| item.def_id)
            .collect::<Vec<_>>();
        for blanket in self.implementations(trait_id)? {
            self.budget.edge(1)?;
            let impl_id = blanket.id();
            let Some(reference) = tcx.impl_opt_trait_ref(impl_id) else {
                continue;
            };
            let reference = reference.instantiate_identity().skip_norm_wip();
            let ty::Param(this) = reference.self_ty().kind() else {
                continue;
            };
            if tcx.impl_polarity(impl_id) != ty::ImplPolarity::Positive {
                continue;
            }
            // The bound's traits `&I: Other`, besides the sysroot's, whose
            // impls for slices Rawr's std gives.
            let mut bounds = Vec::new();
            for (clause, _) in crate::compat::own_clauses(tcx, impl_id) {
                let ty::ClauseKind::Trait(predicate) = clause.kind().skip_binder() else {
                    continue;
                };
                if predicate.polarity == crate::compat::ClausePolarity::Positive
                    && matches!(predicate.self_ty().kind(),
                        ty::Ref(_, target, ty::Mutability::Not)
                            if matches!(target.kind(), ty::Param(parameter) if parameter == this))
                    && !self.is_sysroot_crate(predicate.def_id().krate)?
                {
                    bounds.push(predicate.def_id());
                }
            }
            let mut slices = Vec::new();
            for other in bounds {
                for implementation in self.implementations(other)? {
                    self.budget.edge(1)?;
                    let Some(reference) = tcx.impl_opt_trait_ref(implementation.id()) else {
                        continue;
                    };
                    let reference = reference.instantiate_identity().skip_norm_wip();
                    if matches!(reference.self_ty().kind(),
                        ty::Ref(_, target, ty::Mutability::Not) if target.is_slice())
                        && tcx.impl_polarity(implementation.id()) == ty::ImplPolarity::Positive
                    {
                        slices.push((implementation, other, reference.args));
                    }
                }
            }
            if slices.is_empty() {
                continue;
            }
            for item in &items {
                self.budget.edge(1)?;
                let associated = AssociatedObligation {
                    kind: AssociatedDemandKind::SelectedMember,
                    trait_item: Some(*item),
                    arguments: Some(reference.args),
                    param_env: tcx.param_env(impl_id),
                };
                self.request_implementation(blanket, trait_id, Some(associated))?;
            }
            for (implementation, other, arguments) in slices {
                let types = tcx
                    .associated_items(other)
                    .in_definition_order()
                    .filter(|item| item.is_type())
                    .map(|item| item.def_id)
                    .collect::<Vec<_>>();
                for item in types {
                    self.budget.edge(1)?;
                    let associated = AssociatedObligation {
                        kind: AssociatedDemandKind::SelectedMember,
                        trait_item: Some(item),
                        arguments: Some(arguments),
                        param_env: tcx.param_env(implementation.id()),
                    };
                    self.request_implementation(implementation, other, Some(associated))?;
                }
            }
        }
        Ok(())
    }
}
