//! What one reload did to a running program.

use std::fmt;

/// What a reload did, in the words `rawr-reload` gives a program `rawr
/// run` runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The files hold the text the program runs, or the text a reload
    /// refused last: nothing compiled.
    Unchanged,
    /// The new text compiled. The thread that owns the program takes it at
    /// its next call from native code into script code, and then
    /// [`Outcome::Swapped`] follows.
    Queued,
    /// The program runs the new text now, with its state kept.
    Swapped,
    /// The new text failed: the program keeps running the last good text.
    /// The message is the compile error, as rustc renders it.
    Failed {
        /// The error, as rustc renders it.
        message: String,
    },
    /// The new text can't replace the running one in this process, for
    /// `reason`, such as a script type whose fields changed (D553.6). The
    /// program keeps running the last good text until it starts again.
    Restarting {
        /// Why, in one line.
        reason: String,
    },
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unchanged => f.write_str("unchanged"),
            Self::Queued => f.write_str("queued"),
            Self::Swapped => f.write_str("swapped"),
            Self::Failed { message } => write!(f, "failed: {message}"),
            Self::Restarting { reason } => write!(f, "restarting: {reason}"),
        }
    }
}
