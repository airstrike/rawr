//! Owner-scoped proof that one immutable candidate may replace another.

use std::fmt;
use std::sync::Arc;

use crate::{preview, program, reload};

/// The live state owner whose compatibility must be proven.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Owner {
    /// The generation's compiler-selected application.
    Application,
    /// One compiler-selected preview fixture and callback contract.
    Preview(preview::Key),
}

/// One exact nonempty set of live owners crossing a replacement together.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Owners(Box<[Owner]>);

/// A live-owner set could not establish deterministic replacement scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// At least one live owner is required for compatibility comparison.
    Empty,
    /// The same live owner was supplied more than once.
    Duplicate,
}

/// State action proven for a validated replacement candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    /// Preserve this owner's state and instance under the new generation.
    Retain,
    /// Recreate this owner through its candidate boot contract and rotate instance.
    Reset,
}

/// One exact reason incompatible state cannot cross the replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    /// The selected owner is absent from the current generation.
    MissingCurrentOwner,
    /// The selected owner is absent from the candidate generation.
    MissingCandidateOwner,
    /// Stable reachable nominal identities or exact schemas differ.
    Schema,
    /// The selected callback, facade, or protocol contract differs.
    Callback,
    /// An opaque retained carrier's complete symbolic callable set differs.
    Symbolic,
    /// Scripted-widget lifecycle or tree-state contracts differ.
    Widget,
}

/// Structured evidence produced by one complete compatibility comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    current: program::GenerationId,
    candidate: program::GenerationId,
    owner: Owner,
    action: Action,
    reasons: Box<[Reason]>,
    opaque: bool,
}

/// A candidate paired with the exact current generation and replacement proof.
///
/// Construction consumes the candidate. A later installer can consume this
/// bundle without re-running compatibility or accepting an unproven candidate.
pub struct Plan {
    current: Arc<reload::Generation>,
    candidate: reload::Candidate,
    evidence: Box<[Evidence]>,
}

impl Plan {
    /// Compare one finished candidate against the exact current owner set.
    #[must_use]
    pub fn compare(
        current: Arc<reload::Generation>,
        candidate: reload::Candidate,
        owners: Owners,
    ) -> Self {
        let evidence = owners
            .iter()
            .cloned()
            .map(|owner| compare(&current, candidate.generation(), owner))
            .collect();
        Self {
            current,
            candidate,
            evidence,
        }
    }

    /// Return the proven action for one owner in this exact set.
    #[must_use]
    pub fn action(&self, owner: &Owner) -> Option<Action> {
        self.evidence(owner).map(Evidence::action)
    }

    /// Return structured comparison evidence in stable owner order.
    #[must_use]
    pub const fn evidence_set(&self) -> &[Evidence] {
        &self.evidence
    }

    /// Return structured comparison evidence for one exact owner.
    #[must_use]
    pub fn evidence(&self, owner: &Owner) -> Option<&Evidence> {
        self.evidence
            .binary_search_by(|evidence| evidence.owner.cmp(owner))
            .ok()
            .map(|index| &self.evidence[index])
    }

    pub(super) fn into_parts(
        self,
    ) -> (Arc<reload::Generation>, reload::Candidate, Box<[Evidence]>) {
        (self.current, self.candidate, self.evidence)
    }
}

impl Owners {
    /// Establish a replacement scope containing exactly one live owner.
    #[must_use]
    pub fn one(owner: Owner) -> Self {
        Self(vec![owner].into_boxed_slice())
    }

    /// Validate, sort, and seal one exact nonempty live-owner set.
    pub fn new(owners: impl IntoIterator<Item = Owner>) -> Result<Self, Error> {
        let mut owners: Vec<_> = owners.into_iter().collect();
        if owners.is_empty() {
            return Err(Error::Empty);
        }
        owners.sort();
        if owners.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::Duplicate);
        }
        Ok(Self(owners.into_boxed_slice()))
    }

    /// Return the exact live owners in stable order.
    #[must_use = "iterators are lazy and must be consumed"]
    pub fn iter(&self) -> std::slice::Iter<'_, Owner> {
        self.0.iter()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("reload compatibility requires a live owner"),
            Self::Duplicate => formatter.write_str("reload compatibility owner is duplicated"),
        }
    }
}

impl std::error::Error for Error {}

fn compare(current: &reload::Generation, candidate: &reload::Generation, owner: Owner) -> Evidence {
    let current_contract = contract(current, &owner);
    let candidate_contract = contract(candidate, &owner);
    let mut reasons = Vec::new();
    match (current_contract, candidate_contract) {
        (None, None) => {
            reasons.push(Reason::MissingCurrentOwner);
            reasons.push(Reason::MissingCandidateOwner);
        }
        (None, Some(_)) => reasons.push(Reason::MissingCurrentOwner),
        (Some(_), None) => reasons.push(Reason::MissingCandidateOwner),
        (Some(current), Some(candidate)) => {
            if current.schemas != candidate.schemas {
                reasons.push(Reason::Schema);
            }
            if current.callback != candidate.callback {
                reasons.push(Reason::Callback);
            }
            if current.items != candidate.items {
                reasons.push(Reason::Symbolic);
            }
            if current.widgets != candidate.widgets {
                reasons.push(Reason::Widget);
            }
        }
    }
    let opaque = current_contract.is_some_and(|contract| contract.schemas.is_opaque())
        || candidate_contract.is_some_and(|contract| contract.schemas.is_opaque());
    let action = if reasons.is_empty() {
        Action::Retain
    } else {
        Action::Reset
    };
    Evidence {
        current: current.id(),
        candidate: candidate.id(),
        owner,
        action,
        reasons: reasons.into_boxed_slice(),
        opaque,
    }
}

impl Evidence {
    /// Return the exact current executable generation.
    #[must_use]
    pub const fn current(&self) -> program::GenerationId {
        self.current
    }

    /// Return the exact candidate executable generation.
    #[must_use]
    pub const fn candidate(&self) -> program::GenerationId {
        self.candidate
    }

    /// Return the selected state owner.
    #[must_use]
    pub const fn owner(&self) -> &Owner {
        &self.owner
    }

    /// Return the proven state action.
    #[must_use]
    pub const fn action(&self) -> Action {
        self.action
    }

    /// Return every exact incompatibility reason in comparison order.
    #[must_use]
    pub const fn reasons(&self) -> &[Reason] {
        &self.reasons
    }

    /// Return whether unaudited retained host content widened comparison.
    #[must_use]
    pub const fn is_opaque(&self) -> bool {
        self.opaque
    }
}

impl fmt::Debug for Plan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Plan")
            .field("evidence", &self.evidence)
            .finish_non_exhaustive()
    }
}

fn contract<'a>(
    generation: &'a reload::Generation,
    owner: &Owner,
) -> Option<&'a super::callback::Contract> {
    match owner {
        Owner::Application => generation.callbacks().application_contract(),
        Owner::Preview(key) => generation.callbacks().preview_contract(key),
    }
}
