//! One original contribution shared by executable and closure-captured support.

use std::sync::{Arc, OnceLock};

use crate::host::ImportAllocation;
use rawr_import::surface::Error;

use super::{Attempt, Receipt};

/// Private construction retains the attempt until its single completion.
pub(crate) struct Pending {
    allocation: Attempt,
    owner: Owner,
}

/// Cloning an owner retains the same contribution without refunding any part.
/// The cell never points back to an executable, AST, module or support model.
#[derive(Clone)]
pub(crate) struct Owner(Arc<OnceLock<Receipt>>);

/// Completion is consumed by the executable that was lowered with this owner.
pub(crate) struct Completed {
    owner: Owner,
}

impl Pending {
    pub(crate) fn transform(allocation: Attempt) -> Self {
        Self {
            allocation,
            owner: Owner(Arc::new(OnceLock::new())),
        }
    }

    pub(crate) fn finish(self) -> Completed {
        let Self { allocation, owner } = self;
        owner
            .0
            .set(allocation.finish())
            .expect("only the consumed support construction can complete its receipt");
        Completed { owner }
    }
}

impl Completed {
    pub(crate) fn require(self, owner: &Owner) {
        assert!(
            Arc::ptr_eq(&self.owner.0, &owner.0),
            "support completion must belong to the same lowered executable",
        );
    }
}

impl ImportAllocation for Pending {
    fn retain_checked(&mut self, items: Option<usize>, bytes: Option<usize>) -> Result<(), Error> {
        self.allocation.retain_checked(items, bytes)
    }

    fn support_owner(&self) -> Option<Owner> {
        Some(self.owner.clone())
    }
}

#[cfg(test)]
mod tests;
