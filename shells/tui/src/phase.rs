//! Where the program is, as the session's events tell it.

use std::fmt;

use serde_json::Value;

/// Where the program is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Phase {
    /// Nothing reported yet.
    #[default]
    Starting,
    /// A cold start builds: what its status line says.
    Building(String),
    Running,
    /// Paused at a breakpoint or a step.
    Paused(Stop),
    /// A save restarts it.
    Restarting,
    /// It failed to start; a save that fixes it starts it.
    Failed,
    /// It ended with this status.
    Ended(i64),
    /// The session's connection closed: `rawr run` is gone.
    Gone,
}

/// Where a paused program is: its file, line and function, and why it
/// paused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stop {
    file: String,
    line: usize,
    function: Option<String>,
    reason: Option<String>,
}

impl Stop {
    /// The stop a `paused` event or answer names: `None` without a file
    /// and line.
    #[must_use]
    pub fn from_json(at: &Value) -> Option<Self> {
        Some(Self {
            file: at["file"].as_str()?.to_owned(),
            line: usize::try_from(at["line"].as_u64()?).ok()?,
            function: at["function"].as_str().map(str::to_owned),
            reason: at["reason"].as_str().map(str::to_owned),
        })
    }

    /// The file it is paused in.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The one-based line it is paused at.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// The function it is paused in, when the session names it.
    #[must_use]
    pub fn function(&self) -> Option<&str> {
        self.function.as_deref()
    }

    /// Why it paused, when the session says: a breakpoint, a step or a
    /// pause.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

impl Phase {
    /// The phase after `event`, from this one. An event that moves no
    /// phase leaves it as it is.
    #[must_use]
    pub fn after(&self, event: &Value) -> Self {
        match event["event"].as_str().unwrap_or_default() {
            "building" => Self::Building(event["status"].as_str().unwrap_or_default().to_owned()),
            "running" | "queued" | "swapped" | "resumed" => Self::Running,
            "paused" => Stop::from_json(event).map_or_else(|| self.clone(), Self::Paused),
            "restarting" => Self::Restarting,
            // A failed save leaves a live program as it was.
            "failed" if self.is_live() => self.clone(),
            "failed" => Self::Failed,
            "ended" => Self::Ended(event["status"].as_i64().unwrap_or(0)),
            _ => self.clone(),
        }
    }

    /// Where the program is paused, if it is.
    #[must_use]
    pub const fn stop(&self) -> Option<&Stop> {
        match self {
            Self::Paused(stop) => Some(stop),
            _ => None,
        }
    }

    /// The program is there to answer: running or paused.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        matches!(self, Self::Running | Self::Paused(_))
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Starting => formatter.write_str("starting"),
            Self::Building(status) => write!(formatter, "building: {status}"),
            Self::Running => formatter.write_str("running"),
            Self::Paused(stop) => write!(formatter, "paused {stop}"),
            Self::Restarting => formatter.write_str("restarting"),
            Self::Failed => formatter.write_str("failed"),
            Self::Ended(status) => write!(formatter, "ended with {status}"),
            Self::Gone => formatter.write_str("exited"),
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = std::path::Path::new(&self.file).file_name().map_or_else(
            || self.file.clone(),
            |name| name.to_string_lossy().into_owned(),
        );
        write!(formatter, "at {file}:{}", self.line)?;
        if let Some(function) = &self.function {
            write!(formatter, " in `{function}`")?;
        }
        if let Some(reason) = &self.reason {
            write!(formatter, " ({reason})")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Phase;

    #[test]
    fn events_move_the_phase_and_a_failed_save_keeps_a_paused_program() {
        let running = Phase::Starting.after(&json!({ "event": "running" }));
        assert_eq!(running, Phase::Running);
        let paused = running.after(&json!({
            "event": "paused", "file": "/p/src/main.rs", "line": 9,
            "function": "p::main", "reason": "breakpoint"
        }));
        assert_eq!(
            paused.to_string(),
            "paused at main.rs:9 in `p::main` (breakpoint)"
        );
        assert_eq!(paused.after(&json!({ "event": "failed" })), paused);
        assert_eq!(paused.after(&json!({ "event": "resumed" })), Phase::Running);
        assert_eq!(
            Phase::Starting.after(&json!({ "event": "failed" })),
            Phase::Failed
        );
        assert_eq!(
            Phase::Running.after(&json!({ "event": "ended", "status": 3 })),
            Phase::Ended(3)
        );
    }
}
