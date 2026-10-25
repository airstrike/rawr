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
//! - A render that starts with no other render queued or running may use
//!   the idle budget instead, when its user's allowance has room. Once it
//!   is past the animation budget, a newcomer cuts it at its next frame.
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
/// The idle budget: an animation's wall-clock time when nothing else is
/// queued or running.
pub const IDLE: Duration = Duration::from_secs(60);
/// Animation render time each user has in each [`WINDOW`]: one full idle
/// render and two at the animation budget.
pub const ALLOWANCE: Duration = Duration::from_secs(90);
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
    /// Whether that budget is the idle one, past [`Budgets::animation`].
    pub idle: bool,
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
    /// The idle budget.
    pub idle: Duration,
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
        Self::new(STILL, ANIMATION, IDLE, ALLOWANCE, WINDOW)
    }
}

impl Budgets {
    /// Budgets with these numbers.
    #[must_use]
    pub fn new(
        still: Duration,
        animation: Duration,
        idle: Duration,
        allowance: Duration,
        window: Duration,
    ) -> Self {
        Self {
            still,
            animation,
            idle,
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

    /// Grant a render for `user` at `now`. `alone`: no other render is
    /// queued or running, so it may use the idle budget. A grant with
    /// [`Grant::extended`] holds the longer budget until [`Budgets::finish`].
    pub fn grant(&mut self, user: Option<&str>, now: Instant, alone: bool) -> Grant {
        let still = Grant {
            user: user.map(str::to_owned),
            animation: self.still,
            extended: false,
            idle: false,
            refused: None,
        };
        let Some(key) = user else {
            return still;
        };
        if !self.offers_more() {
            return still;
        }
        let left = self.left(key, now);
        let longest = if alone {
            self.idle.max(self.animation)
        } else {
            self.animation
        };
        let animation = longest.min(left);
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
            idle: animation > self.animation,
            ..still
        }
    }

    /// When a render granted at `started` on the idle budget stops for a
    /// newcomer that arrived at `arrived`: at once when it is past the
    /// animation budget, else when it gets there.
    #[must_use]
    pub fn cut_at(&self, started: Instant, arrived: Instant) -> Instant {
        arrived.max(started + self.animation)
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
            // It may use up the allowance, but never go below it.
            let took = took.min(self.left(user, now));
            self.used
                .entry(user.clone())
                .or_default()
                .push_back((now, took));
        }
        // Forget users with nothing left in the window, so the map stays small.
        let window = self.window;
        self.used.retain(|_, renders| {
            while renders
                .front()
                .is_some_and(|(at, _)| now.duration_since(*at) >= window)
            {
                renders.pop_front();
            }
            !renders.is_empty()
        });
    }

    /// `user`'s animation time left at `now`.
    fn left(&self, user: &str, now: Instant) -> Duration {
        let spent: Duration = self.renders(user, now).map(|(_, took)| took).sum();
        self.allowance.saturating_sub(spent)
    }

    /// `user`'s renders still in the window at `now`, oldest first.
    fn renders<'a>(
        &'a self,
        user: &str,
        now: Instant,
    ) -> impl Iterator<Item = (Instant, Duration)> + 'a {
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

/// The note on a render that used the idle budget.
#[must_use]
pub fn idle_note(idle: Duration) -> String {
    format!(
        "The bot was otherwise idle, so this animation could render for up to {} s.",
        motion::seconds(idle)
    )
}

/// The note to show with a render's image: `note` as the renderer gave it,
/// then why it had only the still budget, when the clock cut its animation.
/// A render that ran past `animation` on the idle budget, and was not cut
/// for someone waiting, says it had the idle budget.
#[must_use]
pub fn annotate(
    note: Option<String>,
    grant: &Grant,
    animation: Duration,
    took: Duration,
) -> Option<String> {
    let cut = note
        .as_deref()
        .is_some_and(|note| note.lines().any(|line| line.starts_with(motion::STOPPED)));
    let waited = note
        .as_deref()
        .is_some_and(|note| note.contains(motion::WAITING));
    let extra = match grant.refused {
        Some(refused) if cut => Some(refused_note(refused, animation)),
        _ if grant.idle && took > animation && !waited => Some(idle_note(grant.animation)),
        _ => None,
    };
    match (note, extra) {
        (Some(note), Some(extra)) => Some(format!("{note}\n{extra}")),
        (note, extra) => note.or(extra),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The budgets with a 45 s allowance, which the older tests count on.
    fn budgets() -> Budgets {
        Budgets::new(STILL, ANIMATION, IDLE, secs(45), WINDOW)
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn budget_a_user_gets_the_animation_budget() {
        let mut budgets = budgets();
        let now = Instant::now();
        let grant = budgets.grant(Some("discord:1"), now, false);
        assert_eq!(grant.animation, ANIMATION);
        assert!(grant.extended && grant.refused.is_none());
    }

    #[test]
    fn budget_no_user_gets_the_still_budget() {
        let mut budgets = budgets();
        let grant = budgets.grant(None, Instant::now(), false);
        assert_eq!(
            (grant.animation, grant.extended, grant.refused),
            (STILL, false, None)
        );
    }

    #[test]
    fn budget_one_extended_render_at_a_time() {
        let mut budgets = budgets();
        let now = Instant::now();
        let first = budgets.grant(Some("discord:1"), now, false);
        let second = budgets.grant(Some("discord:2"), now, false);
        assert_eq!(second.animation, STILL);
        assert_eq!(second.refused, Some(Refused::Busy));
        // A still render frees nothing; the extended one does.
        budgets.finish(&second, secs(1), now);
        assert_eq!(
            budgets.grant(Some("web:a"), now, false).refused,
            Some(Refused::Busy)
        );
        budgets.finish(&first, secs(3), now + secs(3));
        let third = budgets.grant(Some("discord:2"), now + secs(3), false);
        assert!(third.extended, "{third:?}");
    }

    #[test]
    fn budget_the_allowance_window_rolls() {
        let mut budgets = budgets();
        let start = Instant::now();
        let mut now = start;
        // Three 15 s renders use up 45 s.
        for _ in 0..3 {
            let grant = budgets.grant(Some("discord:1"), now, false);
            assert!(grant.extended, "{grant:?}");
            now += secs(15);
            budgets.finish(&grant, secs(15), now);
        }
        let refused = budgets.grant(Some("discord:1"), now, false);
        assert_eq!(refused.animation, STILL);
        // The first render ended at 15 s; it leaves the window at 615 s.
        let frees_in = WINDOW + secs(15) - (now - start);
        assert_eq!(refused.refused, Some(Refused::Allowance { frees_in }));
        // Another user is not affected.
        let other = budgets.grant(Some("discord:2"), now, false);
        assert!(other.extended);
        budgets.finish(&other, secs(2), now);
        // Once the first render leaves the window, 15 s is back.
        let later = start + secs(15) + WINDOW;
        let back = budgets.grant(Some("discord:1"), later, false);
        assert_eq!((back.animation, back.extended), (secs(15), true));
    }

    #[test]
    fn budget_renders_within_the_still_budget_are_free() {
        let mut budgets = budgets();
        let now = Instant::now();
        for _ in 0..100 {
            let grant = budgets.grant(Some("web:a"), now, false);
            assert!(grant.extended);
            budgets.finish(&grant, STILL, now);
        }
        assert_eq!(
            budgets.grant(Some("web:a"), now, false).animation,
            ANIMATION
        );
    }

    #[test]
    fn budget_a_partial_allowance_shortens_the_budget() {
        let mut budgets = budgets();
        let now = Instant::now();
        let grant = budgets.grant(Some("discord:1"), now, false);
        budgets.finish(&grant, secs(37), now);
        // 8 s left: more than the still budget, less than the animation one.
        let grant = budgets.grant(Some("discord:1"), now, false);
        assert_eq!((grant.animation, grant.extended), (secs(8), true));
        budgets.finish(&grant, secs(6), now);
        // 2 s left: no more than the still budget, so refused.
        let grant = budgets.grant(Some("discord:1"), now, false);
        assert_eq!(grant.refused, Some(Refused::Allowance { frees_in: WINDOW }));
    }

    #[test]
    fn budget_notes() {
        let cut = "Rendering stopped after 5 s, so the animation shows its first 0.8 s.".to_owned();
        let allowance = Grant {
            user: Some("discord:1".into()),
            animation: STILL,
            extended: false,
            idle: false,
            refused: Some(Refused::Allowance {
                frees_in: Duration::from_millis(250_500),
            }),
        };
        assert_eq!(
            annotate(Some(cut.clone()), &allowance, ANIMATION, STILL).unwrap(),
            format!("{cut}\nYour animation time is used up for now. It frees up in 5 minutes.")
        );
        let busy = Grant {
            refused: Some(Refused::Busy),
            ..allowance.clone()
        };
        assert_eq!(
            annotate(Some(cut.clone()), &busy, ANIMATION, STILL).unwrap(),
            format!(
                "{cut}\nAnother animation was rendering at the same time. Run it again shortly for up to 15 s."
            )
        );
        // A note that is not a clock cut, or none, gets no reason.
        let capped = "Animation shown for the first 5 s.".to_owned();
        assert_eq!(
            annotate(Some(capped.clone()), &busy, ANIMATION, STILL),
            Some(capped)
        );
        assert_eq!(annotate(None, &busy, ANIMATION, STILL), None);
        assert_eq!(wait(Duration::from_secs(40)), "40 seconds");
        assert_eq!(wait(Duration::from_secs(60)), "1 minute");
        assert_eq!(wait(Duration::from_millis(1)), "1 second");
    }

    #[test]
    fn budget_an_idle_bot_gives_the_idle_budget() {
        let mut budgets = Budgets::default();
        let now = Instant::now();
        let grant = budgets.grant(Some("discord:1"), now, true);
        assert_eq!(
            (grant.animation, grant.extended, grant.idle),
            (IDLE, true, true)
        );
        budgets.finish(&grant, secs(1), now + secs(1));
        // With others queued or running, the animation budget.
        let grant = budgets.grant(Some("discord:1"), now + secs(1), false);
        assert_eq!((grant.animation, grant.idle), (ANIMATION, false));
        budgets.finish(&grant, secs(1), now + secs(2));
        // No user: the still budget, idle or not.
        let grant = budgets.grant(None, now + secs(2), true);
        assert_eq!(
            (grant.animation, grant.extended, grant.idle),
            (STILL, false, false)
        );
    }

    #[test]
    fn budget_the_idle_budget_fits_the_allowance_left() {
        let mut budgets = Budgets::default();
        let now = Instant::now();
        let grant = budgets.grant(Some("discord:1"), now, true);
        budgets.finish(&grant, secs(60), now + secs(60));
        // 30 s left: more than the animation budget, so still idle.
        let now = now + secs(60);
        let grant = budgets.grant(Some("discord:1"), now, true);
        assert_eq!((grant.animation, grant.idle), (secs(30), true));
        budgets.finish(&grant, secs(16), now + secs(16));
        // 14 s left: less than the animation budget, so not idle.
        let now = now + secs(16);
        let grant = budgets.grant(Some("discord:1"), now, true);
        assert_eq!(
            (grant.animation, grant.extended, grant.idle),
            (secs(14), true, false)
        );
        budgets.finish(&grant, secs(10), now + secs(10));
        // 4 s left: refused.
        let grant = budgets.grant(Some("discord:1"), now + secs(10), true);
        assert!(
            matches!(grant.refused, Some(Refused::Allowance { .. })),
            "{grant:?}"
        );
    }

    #[test]
    fn budget_an_idle_render_takes_the_allowance_to_zero_not_below() {
        let mut budgets = Budgets::default();
        let start = Instant::now();
        let grant = budgets.grant(Some("discord:1"), start, false);
        budgets.finish(&grant, secs(15), start + secs(15));
        let now = start + secs(15);
        // 75 s left; the idle render runs its 60 s and then encodes for 20.
        let grant = budgets.grant(Some("discord:1"), now, true);
        assert_eq!(grant.animation, IDLE);
        budgets.finish(&grant, secs(80), now + secs(80));
        let now = now + secs(80);
        assert_eq!(budgets.left("discord:1", now), Duration::ZERO);
        // Only 75 s was counted: once the first render leaves the window,
        // 15 s is back, and no more.
        let later = start + secs(15) + WINDOW;
        assert_eq!(budgets.left("discord:1", later), secs(15));
    }

    #[test]
    fn budget_a_newcomer_cuts_an_idle_render_past_the_animation_budget() {
        let budgets = Budgets::default();
        let started = Instant::now();
        // Arriving at 30 s: cut at once.
        assert_eq!(
            budgets.cut_at(started, started + secs(30)),
            started + secs(30)
        );
        // Arriving at 4 s: cut when it reaches 15 s.
        assert_eq!(
            budgets.cut_at(started, started + secs(4)),
            started + ANIMATION
        );
    }

    #[test]
    fn budget_idle_notes() {
        let idle = Grant {
            user: Some("discord:1".into()),
            animation: IDLE,
            extended: true,
            idle: true,
            refused: None,
        };
        let said = idle_note(IDLE);
        assert_eq!(
            said,
            "The bot was otherwise idle, so this animation could render for up to 60 s."
        );
        // Within the animation budget: nothing to say.
        assert_eq!(annotate(None, &idle, ANIMATION, secs(10)), None);
        // Past it: said, after any note from the renderer.
        assert_eq!(
            annotate(None, &idle, ANIMATION, secs(40)),
            Some(said.clone())
        );
        let stopped = motion::stopped_note(IDLE, Duration::from_millis(2300));
        assert_eq!(
            annotate(Some(stopped.clone()), &idle, ANIMATION, secs(70)),
            Some(format!("{stopped}\n{said}"))
        );
        // Cut for someone waiting: that note is enough.
        let cut = format!(
            "{} {}",
            motion::stopped_note(secs(21), secs(1)),
            motion::WAITING
        );
        assert_eq!(
            annotate(Some(cut.clone()), &idle, ANIMATION, secs(25)),
            Some(cut)
        );
    }
}
