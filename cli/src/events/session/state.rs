//! Where a session's program is, from the events that told it. The owner
//! keeps one, and each shell keeps its own from the events it hears, so
//! both read the same phase.

use serde_json::Value;

/// Where a session's program is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Spawned; nothing reported yet.
    #[default]
    Starting,
    /// A cold start builds: its stage and percent.
    Building,
    /// The program runs.
    Running,
    /// The program is paused at a breakpoint or a step (#535); where is in
    /// the session's answer.
    Paused,
    /// A save restarts it.
    Restarting,
    /// It failed to start; a save that fixes it starts it.
    Failed,
    /// It ended with this status. One that failed runs again on a save; one
    /// that exited with status 0 ended `rawr run` too, which [`Self::Exited`]
    /// follows.
    Ended(i32),
    /// `rawr run` itself exited, with this status when the shell knows it.
    Exited(Option<i32>),
    /// The shell that started it stopped it.
    Stopped,
}

impl Phase {
    /// The phase's name, as tools report it.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Building => "building",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Restarting => "restarting",
            Self::Failed => "failed",
            Self::Ended(_) => "ended",
            Self::Exited(_) => "exited",
            Self::Stopped => "stopped",
        }
    }

    /// Still on its way to running or failing.
    #[must_use]
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Starting | Self::Building | Self::Restarting)
    }

    /// Over: nothing more will happen.
    #[must_use]
    pub const fn is_over(&self) -> bool {
        matches!(self, Self::Exited(_) | Self::Stopped)
    }

    /// The program is there to answer: running or paused.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

/// A session's program: its phase and the events that moved it, each with
/// the number the owner gave it.
#[derive(Debug, Default)]
pub struct State {
    phase: Phase,
    events: Vec<(u64, Value)>,
    /// The latest building stage and percent.
    building: Option<(String, u64)>,
    /// The latest failure while a program kept running, cleared by the
    /// next good reload.
    failure: Option<Value>,
    /// Where the program is paused: its file, line, function and reason.
    paused: Option<Value>,
}

impl State {
    /// The current phase.
    #[must_use]
    pub const fn phase(&self) -> &Phase {
        &self.phase
    }

    /// Where the program is paused, if it is.
    #[must_use]
    pub const fn paused(&self) -> Option<&Value> {
        self.paused.as_ref()
    }

    /// The latest building stage and percent.
    #[must_use]
    pub fn building(&self) -> Option<(&str, u64)> {
        self.building
            .as_ref()
            .map(|(stage, percent)| (stage.as_str(), *percent))
    }

    /// The failure the running program carries, if a save failed.
    #[must_use]
    pub const fn failure(&self) -> Option<&Value> {
        self.failure.as_ref()
    }

    /// The events so far, each with its number, oldest first.
    #[must_use]
    pub fn events(&self) -> &[(u64, Value)] {
        &self.events
    }

    /// The number the next event takes.
    #[must_use]
    pub fn next_number(&self) -> u64 {
        self.events.last().map_or(1, |(seq, _)| seq + 1)
    }

    /// A shell's own phase, which no event of the owner's says: `rawr
    /// run` exited, or its shell stopped it.
    pub fn end(&mut self, phase: Phase) {
        if !self.phase.is_over() {
            self.phase = phase;
        }
    }

    /// The program answered a debugger request: paused at `paused`, or
    /// running. Its own `paused` and `resumed` events say the same once
    /// they come.
    pub fn observe_pause(&mut self, paused: Option<Value>) {
        if !self.phase.is_live() {
            return;
        }
        self.phase = if paused.is_some() {
            Phase::Paused
        } else {
            Phase::Running
        };
        self.paused = paused;
    }

    /// Take event number `seq`.
    pub fn record(&mut self, seq: u64, event: Value) {
        let name = event["event"].as_str().unwrap_or_default();
        if name != "paused" && name != "failed" {
            self.paused = None;
        }
        match name {
            "building" => {
                self.phase = Phase::Building;
                self.building = Some((
                    event["stage"].as_str().unwrap_or_default().to_owned(),
                    event["percent"].as_u64().unwrap_or(0),
                ));
            }
            "running" | "queued" | "swapped" => {
                self.phase = Phase::Running;
                self.failure = None;
            }
            // The program's own debugger says where it paused, and that it
            // went on (#535).
            "paused" => {
                self.phase = Phase::Paused;
                let mut at = event.clone();
                if let Some(at) = at.as_object_mut() {
                    at.remove("event");
                    at.remove("seq");
                }
                self.paused = Some(at);
            }
            "resumed" => self.phase = Phase::Running,
            "restarting" => self.phase = Phase::Restarting,
            "failed" => {
                if self.phase.is_live() {
                    self.failure = Some(event.clone());
                } else {
                    self.phase = Phase::Failed;
                }
            }
            "ended" => self.phase = Phase::Ended(event["status"].as_i64().unwrap_or(0) as i32),
            _ => {}
        }
        if self.phase.is_over() {
            return;
        }
        self.events.push((seq, event));
    }
}

#[cfg(test)]
mod tests {
    use super::{Phase, State};
    use serde_json::json;

    fn recorded(state: &mut State, event: serde_json::Value) {
        let seq = state.next_number();
        state.record(seq, event);
    }

    #[test]
    fn events_move_the_phase_and_a_failure_keeps_the_running_program() {
        let mut state = State::default();
        recorded(
            &mut state,
            json!({ "event": "building", "stage": "building API", "percent": 40 }),
        );
        assert_eq!(state.phase, Phase::Building);
        recorded(&mut state, json!({ "event": "running" }));
        assert_eq!(state.phase, Phase::Running);
        recorded(&mut state, json!({ "event": "failed", "diagnostics": [] }));
        assert_eq!(state.phase, Phase::Running);
        assert!(state.failure.is_some());
        recorded(&mut state, json!({ "event": "swapped" }));
        assert!(state.failure.is_none());
        recorded(&mut state, json!({ "event": "ended", "status": 3 }));
        assert_eq!(state.phase, Phase::Ended(3));
        assert_eq!(
            state.events.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
        let mut paused = State::default();
        recorded(&mut paused, json!({ "event": "running" }));
        recorded(
            &mut paused,
            json!({
                "event": "paused", "file": "/p/src/main.rs", "line": 9,
                "function": "p::main", "reason": "breakpoint"
            }),
        );
        assert_eq!(paused.phase, Phase::Paused);
        assert_eq!(paused.phase.name(), "paused");
        assert_eq!(paused.paused.as_ref().unwrap()["line"], 9);
        assert!(paused.paused.as_ref().unwrap().get("event").is_none());
        // A failed save leaves it paused where it was.
        recorded(&mut paused, json!({ "event": "failed", "diagnostics": [] }));
        assert_eq!(paused.phase, Phase::Paused);
        assert!(paused.failure.is_some());
        recorded(&mut paused, json!({ "event": "resumed" }));
        assert_eq!(paused.phase, Phase::Running);
        assert!(paused.paused.is_none());
        recorded(
            &mut paused,
            json!({ "event": "paused", "file": "/p/src/main.rs", "line": 3 }),
        );
        recorded(
            &mut paused,
            json!({ "event": "restarting", "reason": "a save" }),
        );
        assert_eq!(paused.phase, Phase::Restarting);
        assert!(paused.paused.is_none());
        let mut failing = State::default();
        recorded(
            &mut failing,
            json!({ "event": "failed", "diagnostics": [] }),
        );
        assert_eq!(failing.phase, Phase::Failed);
    }
}
