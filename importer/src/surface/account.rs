//! Accounting-target records for the self-contained surface, profile,
//! supervisor and bridge allocation families.
//!
//! These families compare derived sizes against `cargo::Limits` accounting
//! fields (`items`, `json_bytes`, `string_bytes`) without any larger account in
//! scope. Each construction owns one [`Account`]: crossing a target records the
//! axis, target and truthful cumulative amount on the record and publishes it
//! to the process registry; the work continues. Nothing here constructs an
//! error or latches. Refusal in these families is reserved for physical causes:
//! `try_reserve` failure, checked-arithmetic overflow, recursion depth and the
//! node content or IPC frame bounds.

use crate::exceedance;

/// One self-contained allocation's exceedance record and registry identity.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct Account {
    kind: exceedance::Account,
    id: u64,
    record: exceedance::Record,
}

impl Account {
    /// Start one account of the given kind with a process-unique identity.
    #[must_use]
    pub(crate) fn new(kind: exceedance::Account) -> Self {
        Self {
            kind,
            id: exceedance::next_id(),
            record: exceedance::Record::new(),
        }
    }

    /// Start one symbolic surface account.
    #[must_use]
    pub(crate) fn surface() -> Self {
        Self::new(exceedance::Account::Surface)
    }

    /// Start one profile account.
    #[must_use]
    pub(crate) fn profile() -> Self {
        Self::new(exceedance::Account::Profile)
    }

    /// Start one supervisor snapshot account.
    #[must_use]
    pub(crate) fn supervisor() -> Self {
        Self::new(exceedance::Account::Supervisor)
    }

    /// Start one bridge generation account.
    #[must_use]
    pub(crate) fn bridge() -> Self {
        Self::new(exceedance::Account::Bridge)
    }

    /// Record one measured amount against its accounting target. Exactly at
    /// the target admits silently; one over records the axis with the truthful
    /// amount and publishes the account. The caller continues its work.
    pub(crate) fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target
            && self
                .record
                .exceeded(resource, target as u64, actual as u64, None)
        {
            exceedance::publish(self.kind, self.id, &self.record);
        }
    }

    /// The axes of this account that crossed their targets so far.
    #[must_use]
    pub const fn exceedances(&self) -> exceedance::Record {
        self.record
    }

    /// Process-unique identity of this account.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }
}

#[cfg(test)]
mod tests {
    use super::Account;

    #[test]
    fn exact_admits_silently_and_one_over_records_truthful_high_water() {
        let mut account = Account::surface();
        account.exceeded("fixture bytes", 8, 8);
        assert!(!account.exceedances().is_exceeded());
        account.exceeded("fixture bytes", 8, 9);
        account.exceeded("fixture bytes", 8, 12);
        account.exceeded("fixture bytes", 8, 10);
        let axis = *account
            .exceedances()
            .axis("fixture bytes")
            .expect("one over records the axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (8, 9, 12)
        );
        let published =
            crate::exceedance::published(crate::exceedance::Account::Surface, account.id());
        if let Some(published) = published {
            assert_eq!(published.record, account.exceedances());
        }
    }
}
