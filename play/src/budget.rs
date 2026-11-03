//! How long the bot lets a run go on. A run has no deadline while no other
//! snippet waits for the server: it may take as long as it needs, up to a
//! ceiling against a runaway loop. Once another snippet waits, the run gets
//! the normal budget from that moment.

use std::time::{Duration, Instant};

/// The bot's time settings for one run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Budget {
    time: Duration,
    timeout: Duration,
    max: Duration,
}

impl Budget {
    /// `time` of script time (`PLAY_TIME_MS`) and `timeout` for the whole
    /// process (`PLAY_TIMEOUT_MS`) once another snippet waits, and `max` of
    /// script time (`PLAY_MAX_MS`) while none does. The whole process gets
    /// `max` plus the time `timeout` leaves past `time`.
    #[must_use]
    pub fn new(time: Duration, timeout: Duration, max: Duration) -> Self {
        Self { time, timeout, max }
    }

    /// The script time a run is started with: the ceiling.
    #[must_use]
    pub fn max(&self) -> Duration {
        self.max
    }

    /// The whole time a run gets once another snippet waits.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// When a run started at `started` stops its script: the ceiling, or
    /// `time` after `contended`, the moment another snippet began to wait.
    #[must_use]
    pub fn script(&self, started: Instant, contended: Option<Instant>) -> Instant {
        let ceiling = started + self.max;
        contended.map_or(ceiling, |at| ceiling.min(at + self.time))
    }

    /// When the process of a run started at `started` is killed, as
    /// [`Budget::script`] with `timeout`.
    #[must_use]
    pub fn kill(&self, started: Instant, contended: Option<Instant>) -> Instant {
        let ceiling = started + self.max + self.timeout.saturating_sub(self.time);
        contended.map_or(ceiling, |at| ceiling.min(at + self.timeout))
    }
}

#[cfg(test)]
mod tests {
    use super::Budget;
    use std::time::{Duration, Instant};

    fn budget() -> Budget {
        Budget::new(
            Duration::from_secs(60),
            Duration::from_secs(120),
            Duration::from_secs(600),
        )
    }

    #[test]
    fn an_idle_run_has_only_the_ceiling() {
        let started = Instant::now();
        let budget = budget();
        assert_eq!(
            budget.script(started, None),
            started + Duration::from_secs(600)
        );
        assert_eq!(
            budget.kill(started, None),
            started + Duration::from_secs(660)
        );
    }

    #[test]
    fn a_waiting_snippet_starts_the_budget_from_its_arrival() {
        let started = Instant::now();
        let arrived = started + Duration::from_secs(200);
        let budget = budget();
        assert_eq!(
            budget.script(started, Some(arrived)),
            arrived + Duration::from_secs(60)
        );
        assert_eq!(
            budget.kill(started, Some(arrived)),
            arrived + Duration::from_secs(120)
        );
        // One waiting at the start: the budget from the start.
        assert_eq!(
            budget.script(started, Some(started)),
            started + Duration::from_secs(60)
        );
    }

    #[test]
    fn a_snippet_arriving_near_the_ceiling_leaves_the_ceiling() {
        let started = Instant::now();
        let arrived = started + Duration::from_secs(590);
        let budget = budget();
        assert_eq!(
            budget.script(started, Some(arrived)),
            started + Duration::from_secs(600)
        );
        assert_eq!(
            budget.kill(started, Some(arrived)),
            started + Duration::from_secs(660)
        );
    }
}
