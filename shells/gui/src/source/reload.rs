//! What a save did, from the session's events: rawr's run takes a save as
//! a swap in place, a restart or an error, whoever saved.

use serde_json::Value;

/// What the last save did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reload {
    /// The window wrote the file, and the run has yet to say.
    Saving,
    /// The run builds the save in full before it goes in.
    Preparing,
    /// The program takes the save at its next call into script code.
    Queued,
    /// The save went into the running program, which kept its state.
    Swapped,
    /// The save restarted the program.
    Restarted,
    /// The save failed: the first error, and where.
    Failed(Failure),
}

/// A save's first error and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    /// The file, as the run names it: relative to where it runs.
    pub file: Option<String>,
    /// Its line, from 1.
    pub line: Option<u64>,
}

impl Reload {
    /// What `event`, one of the session's, says a save did, if it says.
    #[must_use]
    pub fn of(event: &Value) -> Option<Self> {
        match event["event"].as_str()? {
            "preparing" => Some(Self::Preparing),
            "queued" => Some(Self::Queued),
            "swapped" => Some(Self::Swapped),
            "restarting" => Some(Self::Restarted),
            "failed" => Some(Self::Failed(Failure {
                message: event["message"]
                    .as_str()
                    .unwrap_or("the save failed")
                    .to_owned(),
                file: event["file"].as_str().map(str::to_owned),
                line: event["line"].as_u64(),
            })),
            _ => None,
        }
    }

    /// The failure's line in `shown`, the file shown, when it is in it.
    #[must_use]
    pub fn failed_at(&self, shown: &str) -> Option<(u64, &str)> {
        let Self::Failed(failure) = self else {
            return None;
        };
        let file = failure.file.as_deref()?;
        let same = shown == file
            || shown
                .strip_suffix(file)
                .is_some_and(|parent| parent.ends_with('/'));
        same.then_some((failure.line?, failure.message.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Reload;

    #[test]
    fn a_failed_save_marks_its_line_in_the_file_it_names() {
        let failed = Reload::of(&json!({
            "event": "failed", "message": "expected `;`", "file": "src/main.rs", "line": 4,
        }))
        .unwrap();
        assert_eq!(
            failed.failed_at("/p/src/main.rs"),
            Some((4, "expected `;`"))
        );
        assert_eq!(failed.failed_at("/p/src/lib.rs"), None);
        assert_eq!(failed.failed_at("/p/xsrc/main.rs"), None);
        assert_eq!(
            Reload::of(&json!({ "event": "swapped" })),
            Some(Reload::Swapped)
        );
        assert_eq!(Reload::of(&json!({ "event": "paused" })), None);
    }
}
