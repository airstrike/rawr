//! The opaque facts of the plans a runner renders, found once per rendering:
//! each plan's operand that holds an opaque type and the opaque type it
//! gives, and by opaque type the plans that take it and the one that
//! compiles its table. Every adapter asks them of the whole cohort, so
//! without them each question substitutes every plan's signature again.
//!
//! They answer for the cohort they were found for, the plans the rendering
//! borrows, and only while it holds them ([`Indexed`]): a plan or cohort
//! elsewhere is found as before.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::classify::representability::convert;
use crate::surface::adapter::Plan;

use super::request;

/// A plan's operand that holds an opaque type: its argument index, the
/// opaque type and the operand's whole type (`opaque_operand`).
pub(super) type Operand = (usize, convert::Type, convert::Type);

struct Opaques {
    /// The cohort's first plan's address and its length.
    start: usize,
    len: usize,
    operands: Vec<Option<Operand>>,
    produced: Vec<Option<convert::Type>>,
    /// By opaque type (`opaque_key`), the plans that take a value holding
    /// it, in the cohort's order.
    consumers: BTreeMap<convert::Type, Vec<usize>>,
    /// By opaque type, the first plan that gives a value holding it and
    /// takes none.
    producers: BTreeMap<convert::Type, usize>,
}

thread_local! {
    static COHORT: RefCell<Option<Opaques>> = const { RefCell::new(None) };
}

/// The rendering's hold on its cohort's opaque facts, which go when it ends.
#[must_use]
pub(super) struct Indexed(());

impl Drop for Indexed {
    fn drop(&mut self) {
        COHORT.with_borrow_mut(|cohort| *cohort = None);
    }
}

/// Find the opaque facts of `cohort`, which answer for it while the
/// returned hold lives. The opaque types the rendering declares
/// (`TypePaths::install`) must be in place, as every fact reads them. A
/// cohort with a plan whose facts can't be found has none: each question
/// finds its answer as before, and fails where it did.
pub(super) fn index(cohort: &[Plan]) -> Indexed {
    COHORT.with_borrow_mut(|cohort| *cohort = None);
    if let Ok(found) = Opaques::find(cohort) {
        COHORT.with_borrow_mut(|cohort| *cohort = Some(found));
    }
    Indexed(())
}

impl Opaques {
    fn find(cohort: &[Plan]) -> Result<Self, request::Error> {
        let mut operands = Vec::with_capacity(cohort.len());
        let mut produced = Vec::with_capacity(cohort.len());
        for plan in cohort {
            operands.push(super::find_opaque_operand(plan)?);
            produced.push(super::find_produced_opaque(plan)?);
        }
        let mut consumers = BTreeMap::<convert::Type, Vec<usize>>::new();
        let mut producers = BTreeMap::new();
        for (index, (operand, given)) in operands.iter().zip(&produced).enumerate() {
            match (operand, given) {
                (Some((_, opaque, _)), _) => consumers
                    .entry(super::opaque_key(opaque))
                    .or_default()
                    .push(index),
                (None, Some(given)) => {
                    producers.entry(super::opaque_key(given)).or_insert(index);
                }
                (None, None) => {}
            }
        }
        Ok(Self {
            start: cohort.as_ptr() as usize,
            len: cohort.len(),
            operands,
            produced,
            consumers,
            producers,
        })
    }

    /// The index of `plan` in the cohort, when it is one of its plans.
    fn position(&self, plan: &Plan) -> Option<usize> {
        let size = std::mem::size_of::<Plan>();
        let offset = (plan as *const Plan as usize).checked_sub(self.start)?;
        (offset % size == 0 && offset / size < self.len).then_some(offset / size)
    }

    /// Whether `cohort` is the cohort these facts are of.
    fn is(&self, cohort: &[Plan]) -> bool {
        cohort.as_ptr() as usize == self.start && cohort.len() == self.len
    }
}

/// `plan`'s opaque operand, when it is a plan of the indexed cohort.
pub(super) fn operand(plan: &Plan) -> Option<Option<Operand>> {
    COHORT.with_borrow(|cohort| {
        let cohort = cohort.as_ref()?;
        Some(cohort.operands[cohort.position(plan)?].clone())
    })
}

/// The opaque type `plan` gives, when it is a plan of the indexed cohort.
pub(super) fn produced(plan: &Plan) -> Option<Option<convert::Type>> {
    COHORT.with_borrow(|cohort| {
        let cohort = cohort.as_ref()?;
        Some(cohort.produced[cohort.position(plan)?].clone())
    })
}

/// The plans of `cohort` that take a value holding the opaque type `key`,
/// when it is the indexed cohort.
pub(super) fn consumers<'p>(cohort: &'p [Plan], key: &convert::Type) -> Option<Vec<&'p Plan>> {
    COHORT.with_borrow(|indexed| {
        let indexed = indexed.as_ref().filter(|indexed| indexed.is(cohort))?;
        Some(
            indexed
                .consumers
                .get(&super::opaque_key(key))
                .into_iter()
                .flatten()
                .map(|index| &cohort[*index])
                .collect(),
        )
    })
}

/// The plan of `cohort` that compiles the table of the opaque type `key`,
/// when it is the indexed cohort.
pub(super) fn producer<'p>(cohort: &'p [Plan], key: &convert::Type) -> Option<Option<&'p Plan>> {
    COHORT.with_borrow(|indexed| {
        let indexed = indexed.as_ref().filter(|indexed| indexed.is(cohort))?;
        Some(
            indexed
                .producers
                .get(&super::opaque_key(key))
                .map(|index| &cohort[*index]),
        )
    })
}
