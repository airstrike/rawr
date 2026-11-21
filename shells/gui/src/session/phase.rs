//! Where the session's program is, from the events the session tells.

use serde_json::Value;

/// Where the session's program is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Nothing reported yet.
    #[default]
    Starting,
    /// A cold start builds: what its status line says.
    Building(String),
    /// The program runs.
    Running,
    /// The program is paused at a breakpoint or a step.
    Paused(Stop),
    /// A save restarts it.
    Restarting,
    /// It failed to start; a save that fixes it starts it.
    Failed,
    /// It ended with this status.
    Ended(i64),
    /// The session's connection closed: the run is over.
    Closed,
}

/// Where a paused program stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    /// The file, when the stop is in one of the program's files.
    pub file: Option<String>,
    /// Its line, from 1.
    pub line: Option<u64>,
    /// The function's Rust path.
    pub function: Option<String>,
    /// Why it stopped: a breakpoint or a step.
    pub reason: String,
}

impl Stop {
    /// The stop a `paused` event or a debugger answer's `paused` names, or
    /// `None` for anything that isn't one.
    #[must_use]
    pub fn from_json(paused: &Value) -> Option<Self> {
        paused.as_object()?;
        Some(Self {
            file: paused["file"].as_str().map(str::to_owned),
            line: paused["line"].as_u64(),
            function: paused["function"].as_str().map(str::to_owned),
            reason: paused["reason"].as_str().unwrap_or_default().to_owned(),
        })
    }
}

impl Phase {
    /// The phase's name, as the session names it.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Building(_) => "building",
            Self::Running => "running",
            Self::Paused(_) => "paused",
            Self::Restarting => "restarting",
            Self::Failed => "failed",
            Self::Ended(_) => "ended",
            Self::Closed => "closed",
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

    /// The phase after `event`, one of the session's events.
    #[must_use]
    pub fn after(&self, event: &Value) -> Self {
        if *self == Self::Closed {
            return Self::Closed;
        }
        match event["event"].as_str().unwrap_or_default() {
            "building" => Self::Building(event["status"].as_str().unwrap_or_default().to_owned()),
            "running" | "queued" | "swapped" | "resumed" => Self::Running,
            "paused" => Stop::from_json(event).map_or(Self::Running, Self::Paused),
            "restarting" => Self::Restarting,
            // A failed save leaves a live program where it was.
            "failed" if matches!(self, Self::Running | Self::Paused(_)) => self.clone(),
            "failed" => Self::Failed,
            "ended" => Self::Ended(event["status"].as_i64().unwrap_or(0)),
            _ => self.clone(),
        }
    }

    /// The phase after a debugger answer that says where the program is
    /// paused: the session's own event says the same once it comes. An
    /// answer that names no pause leaves the phase as the events told it,
    /// as a pause it led to may be told before the answer comes.
    #[must_use]
    pub fn answered(&self, paused: &Value) -> Self {
        if !matches!(self, Self::Running | Self::Paused(_)) {
            return self.clone();
        }
        Stop::from_json(paused).map_or_else(|| self.clone(), Self::Paused)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Phase;

    #[test]
    fn events_move_the_phase_and_a_failed_save_keeps_a_pause() {
        let mut phase = Phase::default();
        for event in [
            json!({ "event": "building", "stage": "building API", "status": "building API" }),
            json!({ "event": "running" }),
            json!({ "event": "paused", "file": "/p/main.rs", "line": 3, "reason": "breakpoint" }),
            json!({ "event": "failed" }),
        ] {
            phase = phase.after(&event);
        }
        assert_eq!(phase.stop().and_then(|stop| stop.line), Some(3));
        phase = phase.after(&json!({ "event": "resumed" }));
        assert_eq!(phase, Phase::Running);
        assert_eq!(
            phase.after(&json!({ "event": "ended", "status": 2 })),
            Phase::Ended(2)
        );
    }
}
