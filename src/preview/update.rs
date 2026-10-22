//! Off-side preview update result awaiting host boundary conversion.

use std::fmt;

use crate::Dynamic;

/// A fully type-validated candidate State and optional Task output.
///
/// The caller converts the opaque Task value before committing `state`, so a
/// host-boundary conversion failure cannot expose partial script mutation.
pub struct Update {
    state: Dynamic,
    task: Option<Dynamic>,
}

impl Update {
    pub(crate) fn new(state: Dynamic, task: Option<Dynamic>) -> Self {
        Self { state, task }
    }

    /// Convert the optional Task boundary before exposing the candidate State.
    ///
    /// A failed conversion drops the candidate State and returns only the
    /// conversion error, leaving the caller's active State untouched.
    pub fn try_into_parts<T, E>(
        self,
        convert: impl FnOnce(Dynamic) -> Result<T, E>,
    ) -> Result<(Dynamic, Option<T>), E> {
        let task = self.task.map(convert).transpose()?;
        Ok((self.state, task))
    }
}

impl fmt::Debug for Update {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Update")
            .field("state", &"<private>")
            .field("has_task", &self.task.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Update;

    #[test]
    fn failed_task_conversion_never_releases_candidate_state() {
        let candidate = Update::new(42_i64.into(), Some("bad task".into()));
        let result = candidate.try_into_parts::<(), _>(|_| Err("invalid Task"));

        assert!(matches!(result, Err("invalid Task")));
    }
}
