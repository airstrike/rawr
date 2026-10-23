//! How long a render may run: the still budget, or an animation's longer
//! one, with abuse control.
//!
//! Every render starts on the still budget. A full app whose view changes
//! may record up to the animation budget, when its caller grants it:
//!
//! - Each user (a Discord user, or a playground IP) has an allowance of
//!   animation render time over a rolling window. A render that runs past
//!   the still budget counts its whole wall-clock time against it.
//! - One render at a time holds the longer budget across the bot. Others
//!   get the still budget while it runs.
//!
//! A render refused the longer budget still renders, on the still budget.
//! When the clock then cuts its animation, its note says why.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::motion;

/// The still budget: wall-clock time for any render.
pub const STILL: Duration = Duration::from_secs(5);
/// The animation budget: wall-clock time for a full app once its view
/// changes.
pub const ANIMATION: Duration = Duration::from_secs(15);
/// Animation render time each user has in each [`WINDOW`].
pub const ALLOWANCE: Duration = Duration::from_secs(45);
/// The rolling window [`ALLOWANCE`] is counted over.
pub const WINDOW: Duration = Duration::from_secs(10 * 60);

/// Why a render did not get the animation budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refused {
    /// Its user's allowance is used up. More frees up after `frees_in`.
    Allowance {
        /// When enough of the allowance frees up for a longer render.
        frees_in: Duration,
    },
    /// Another render holds the longer budget.
    Busy,
}

/// What one render may use.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    /// Who it is for; `None` gets the still budget and is never counted.
    pub user: Option<String>,
    /// The animation budget it may use: [`Budgets::still`] when refused.
    pub animation: Duration,
    /// Whether it holds the one longer budget.
    pub extended: bool,
    /// Why it did not get the longer budget, when it asked for one.
    pub refused: Option<Refused>,
}

/// The budgets, each user's animation time, and the one longer budget.
#[derive(Debug)]
pub struct Budgets {
    /// The still budget.
    pub still: Duration,
    /// The animation budget.
    pub animation: Duration,
    /// Animation render time per user per window.
    pub allowance: Duration,
    /// The rolling window.
    pub window: Duration,
    /// Each user's renders past the still budget: when each ended, and
    /// how long it ran.
    used: HashMap<String, VecDeque<(Instant, Duration)>>,
    /// Whether a render holds the longer budget.
    busy: bool,
}

impl Default for Budgets {
    fn default() -> Self {
        Self::new(STILL, ANIMATION, ALLOWANCE, WINDOW)
    }
}

impl Budgets {
    /// Budgets with these numbers.
    #[must_use]
    pub fn new(still: Duration, animation: Duration, allowance: Duration, window: Duration) -> Self {
        Self {
            still,
            animation,
            allowance,
            window,
            used: HashMap::new(),
            busy: false,
        }
    }

    /// Whether the longer budget would give a render more than the still one.
    fn offers_more(&self) -> bool {
        self.animation > self.still
    }

    /// Grant a render for `user` at `now`. A grant with
    /// [`Grant::extended`] holds the longer budget until [`Budgets::finish`].
    pub fn grant(&mut self, user: Option<&str>, now: Instant) -> Grant {
        let still = Grant {
            user: user.map(str::to_owned),
            animation: self.still,
            extended: false,
            refused: None,
        };
        let Some(key) = user else {
            return still;
        };
        if !self.offers_more() {
            return still;
        }
        let left = self.left(key, now);
        let animation = self.animation.min(left);
        if animation <= self.still {
            let frees_in = self.frees_in(key, now);
            return Grant {
                refused: Some(Refused::Allowance { frees_in }),
                ..still
            };
        }
        if self.busy {
            return Grant {
                refused: Some(Refused::Busy),
                ..still
            };
        }
        self.busy = true;
        Grant {
            animation,
            extended: true,
            ..still
        }
    }

    /// A render granted `grant` ended at `now` after `took`: release the
    /// longer budget, and count it when it ran past the still budget.
    pub fn finish(&mut self, grant: &Grant, took: Duration, now: Instant) {
        if !grant.extended {
            return;
        }
        self.busy = false;
        let Some(user) = &grant.user else {
            return;
        };
        if took > self.still {
            self.used
                .entry(user.clone())
                .or_default()
                .push_back((now, took));
        }
        // Forget users with nothing left in the window, so the map stays small.
        let window = self.window;
        self.used.retain(|_, renders| {
            while renders.front().is_some_and(|(at, _)| now.duration_since(*at) >= window) {
                renders.pop_front();
            }
            !renders.is_empty()
        });
    }

    /// `user`'s animation time left at `now`.
    fn left(&self, user: &str, now: Instant) -> Duration {
        let spent: Duration = self
            .renders(user, now)
            .map(|(_, took)| took)
            .sum();
        self.allowance.saturating_sub(spent)
    }

    /// `user`'s renders still in the window at `now`, oldest first.
    fn renders<'a>(&'a self, user: &str, now: Instant) -> impl Iterator<Item = (Instant, Duration)> + 'a {
        let window = self.window;
        self.used
            .get(user)
            .into_iter()
            .flatten()
            .copied()
            .filter(move |(at, _)| now.duration_since(*at) < window)
    }

    /// How long until `user` has more than the still budget left.
    fn frees_in(&self, user: &str, now: Instant) -> Duration {
        let mut left = self.left(user, now);
        for (at, took) in self.renders(user, now) {
            left += took;
            if left > self.still {
                return (at + self.window).saturating_duration_since(now);
            }
        }
        self.window
    }
}

/// The note on why a render had only `still` of wall-clock time.
#[must_use]
pub fn refused_note(refused: Refused, animation: Duration) -> String {
    match refused {
        Refused::Allowance { frees_in } => format!(
            "Your animation time is used up for now. It frees up in {}.",
            wait(frees_in)
        ),
        Refused::Busy => format!(
            "Another animation was rendering at the same time. Run it again shortly for up to {} s.",
            motion::seconds(animation)
        ),
    }
}

/// `time` as a short wait: whole minutes, rounded up, from a minute on;
/// whole seconds below.
fn wait(time: Duration) -> String {
    let seconds = time.as_secs() + u64::from(time.subsec_nanos() > 0);
    if seconds >= 60 {
        let minutes = seconds.div_ceil(60);
        if minutes == 1 {
            "1 minute".to_owned()
        } else {
            format!("{minutes} minutes")
        }
    } else if seconds == 1 {
        "1 second".to_owned()
    } else {
        format!("{seconds} seconds")
    }
}

/// The note to show with a render's image: `note` as the renderer gave it,
/// then why it had only the still budget, when the clock cut its animation.
#[must_use]
pub fn annotate(note: Option<String>, grant: &Grant, animation: Duration) -> Option<String> {
    let cut = note.as_deref().is_some_and(|note| {
        note.lines().any(|line| line.starts_with(motion::STOPPED))
    });
    match (note, grant.refused) {
        (Some(note), Some(refused)) if cut => {
            Some(format!("{note}\n{}", refused_note(refused, animation)))
        }
        (note, _) => note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budgets() -> Budgets {
        Budgets::default()
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn budget_a_user_gets_the_animation_budget() {
        let mut budgets = budgets();
        let now = Instant::now();
        let grant = budgets.grant(Some("discord:1"), now);
        assert_eq!(grant.animation, ANIMATION);
        assert!(grant.extended && grant.refused.is_none());
    }

    #[test]
    fn budget_no_user_gets_the_still_budget() {
        let mut budgets = budgets();
        let grant = budgets.grant(None, Instant::now());
        assert_eq!((grant.animation, grant.extended, grant.refused), (STILL, false, None));
    }

    #[test]
    fn budget_one_extended_render_at_a_time() {
        let mut budgets = budgets();
        let now = Instant::now();
        let first = budgets.grant(Some("discord:1"), now);
        let second = budgets.grant(Some("discord:2"), now);
        assert_eq!(second.animation, STILL);
        assert_eq!(second.refused, Some(Refused::Busy));
        // A still render frees nothing; the extended one does.
        budgets.finish(&second, secs(1), now);
        assert_eq!(budgets.grant(Some("web:a"), now).refused, Some(Refused::Busy));
        budgets.finish(&first, secs(3), now + secs(3));
        let third = budgets.grant(Some("discord:2"), now + secs(3));
        assert!(third.extended, "{third:?}");
    }

    #[test]
    fn budget_the_allowance_window_rolls() {
        let mut budgets = budgets();
        let start = Instant::now();
        let mut now = start;
        // Three 15 s renders use up 45 s.
        for _ in 0..3 {
            let grant = budgets.grant(Some("discord:1"), now);
            assert!(grant.extended, "{grant:?}");
            now += secs(15);
            budgets.finish(&grant, secs(15), now);
        }
        let refused = budgets.grant(Some("discord:1"), now);
        assert_eq!(refused.animation, STILL);
        // The first render ended at 15 s; it leaves the window at 615 s.
        let frees_in = WINDOW + secs(15) - (now - start);
        assert_eq!(refused.refused, Some(Refused::Allowance { frees_in }));
        // Another user is not affected.
        let other = budgets.grant(Some("discord:2"), now);
        assert!(other.extended);
        budgets.finish(&other, secs(2), now);
        // Once the first render leaves the window, 15 s is back.
        let later = start + secs(15) + WINDOW;
        let back = budgets.grant(Some("discord:1"), later);
        assert_eq!((back.animation, back.extended), (secs(15), true));
    }

    #[test]
    fn budget_renders_within_the_still_budget_are_free() {
        let mut budgets = budgets();
        let now = Instant::now();
        for _ in 0..100 {
            let grant = budgets.grant(Some("web:a"), now);
            assert!(grant.extended);
            budgets.finish(&grant, STILL, now);
        }
        assert_eq!(budgets.grant(Some("web:a"), now).animation, ANIMATION);
    }

    #[test]
    fn budget_a_partial_allowance_shortens_the_budget() {
        let mut budgets = budgets();
        let now = Instant::now();
        let grant = budgets.grant(Some("discord:1"), now);
        budgets.finish(&grant, secs(37), now);
        // 8 s left: more than the still budget, less than the animation one.
        let grant = budgets.grant(Some("discord:1"), now);
        assert_eq!((grant.animation, grant.extended), (secs(8), true));
        budgets.finish(&grant, secs(6), now);
        // 2 s left: no more than the still budget, so refused.
        let grant = budgets.grant(Some("discord:1"), now);
        assert_eq!(grant.refused, Some(Refused::Allowance { frees_in: WINDOW }));
    }

    #[test]
    fn budget_notes() {
        let cut = "Rendering stopped after 5 s, so the animation shows its first 0.8 s.".to_owned();
        let allowance = Grant {
            user: Some("discord:1".into()),
            animation: STILL,
            extended: false,
            refused: Some(Refused::Allowance { frees_in: Duration::from_millis(250_500) }),
        };
        assert_eq!(
            annotate(Some(cut.clone()), &allowance, ANIMATION).unwrap(),
            format!("{cut}\nYour animation time is used up for now. It frees up in 5 minutes.")
        );
        let busy = Grant {
            refused: Some(Refused::Busy),
            ..allowance.clone()
        };
        assert_eq!(
            annotate(Some(cut.clone()), &busy, ANIMATION).unwrap(),
            format!(
                "{cut}\nAnother animation was rendering at the same time. Run it again shortly for up to 15 s."
            )
        );
        // A note that is not a clock cut, or none, gets no reason.
        let capped = "Animation shown for the first 5 s.".to_owned();
        assert_eq!(annotate(Some(capped.clone()), &busy, ANIMATION), Some(capped));
        assert_eq!(annotate(None, &busy, ANIMATION), None);
        assert_eq!(wait(Duration::from_secs(40)), "40 seconds");
        assert_eq!(wait(Duration::from_secs(60)), "1 minute");
        assert_eq!(wait(Duration::from_millis(1)), "1 second");
    }
}
