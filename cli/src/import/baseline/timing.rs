//! Closed timing authority for the descriptive and release launch protocols.

use rawr_import::metrics::evidence;

mod sealed {
    pub trait Authority {}
    pub trait Visibility {}
}

/// Closed compile-time selection; callers cannot supply another deadline policy.
pub(super) trait Authority: sealed::Authority + Sized {
    type Visibility: Visibility;

    fn from_envelope(envelope: evidence::Envelope) -> Result<Self, Error>;
    fn window(&self) -> Window;
    fn observe(&self, observed_ns: u64) -> Result<Self::Visibility, Error>;
}

/// A live first-visible proof may emit inert facts, never another mode's authority.
pub(super) trait Visibility: sealed::Visibility {
    fn origin_ns(&self) -> u64;
    fn observed_ns(&self) -> u64;
    fn latency_ns(&self) -> u64;
    fn report(
        &self,
    ) -> Result<rawr_import::metrics::report::timing::Timing, rawr_import::metrics::report::Error>;
}

const RELEASE_VISIBLE_NS: u64 = 15_000_000_000;
const RELEASE_MEDIAN_NS: u64 = 10_000_000_000;

/// Failure to transform a launch origin into governed timing evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Error {
    #[error("the timing authority clock overflowed")]
    Clock,
    #[error("the {mode} window became visible after its absolute deadline")]
    VisibleAfterDeadline { mode: &'static str },
    #[error("the retained release visibility median exceeds ten seconds")]
    ReleaseMedian,
}

/// Mode-erased observer input minted only by a closed timing authority.
#[derive(Clone, Copy)]
pub(super) struct Window {
    deadline_ns: u64,
}

/// Authority for one descriptive launch.
pub(super) struct DescriptiveBaseline {
    envelope: evidence::Envelope,
}

/// Exact descriptive first-visible proof.
pub(super) struct DescriptiveVisibility {
    origin_ns: u64,
    observed_ns: u64,
    latency_ns: u64,
    exceeds_ten_seconds: bool,
    exceeds_fifteen_seconds: bool,
}

/// Authority for one release launch.
pub(super) struct ReleaseOracle {
    origin_ns: u64,
    window: Window,
}

/// Exact release first-visible proof. It has no baseline conversion.
pub(super) struct ReleaseVisibility {
    origin_ns: u64,
    observed_ns: u64,
    latency_ns: u64,
}

/// Closed proof of three warmups and five retained release observations.
pub(super) struct ReleaseSeries {
    warmups: [ReleaseVisibility; 3],
    retained: [ReleaseVisibility; 5],
    median_ns: u64,
}

impl sealed::Authority for DescriptiveBaseline {}
impl sealed::Authority for ReleaseOracle {}
impl sealed::Visibility for DescriptiveVisibility {}
impl sealed::Visibility for ReleaseVisibility {}

impl Authority for DescriptiveBaseline {
    type Visibility = DescriptiveVisibility;

    fn from_envelope(envelope: evidence::Envelope) -> Result<Self, Error> {
        Ok(Self::transform(envelope))
    }

    fn window(&self) -> Window {
        self.window()
    }

    fn observe(&self, observed_ns: u64) -> Result<Self::Visibility, Error> {
        self.observe(observed_ns)
    }
}

impl Authority for ReleaseOracle {
    type Visibility = ReleaseVisibility;

    fn from_envelope(envelope: evidence::Envelope) -> Result<Self, Error> {
        Self::transform(envelope.origin_ns())
    }

    fn window(&self) -> Window {
        self.window
    }

    fn observe(&self, observed_ns: u64) -> Result<Self::Visibility, Error> {
        self.observe(observed_ns)
    }
}

impl Visibility for DescriptiveVisibility {
    fn origin_ns(&self) -> u64 {
        self.origin_ns
    }
    fn observed_ns(&self) -> u64 {
        self.observed_ns
    }
    fn latency_ns(&self) -> u64 {
        self.latency_ns
    }
    fn report(
        &self,
    ) -> Result<rawr_import::metrics::report::timing::Timing, rawr_import::metrics::report::Error>
    {
        rawr_import::metrics::report::timing::Timing::descriptive_baseline(
            self.origin_ns,
            self.observed_ns,
        )
    }
}

impl Visibility for ReleaseVisibility {
    fn origin_ns(&self) -> u64 {
        self.origin_ns
    }
    fn observed_ns(&self) -> u64 {
        self.observed_ns
    }
    fn latency_ns(&self) -> u64 {
        self.latency_ns
    }
    fn report(
        &self,
    ) -> Result<rawr_import::metrics::report::timing::Timing, rawr_import::metrics::report::Error>
    {
        rawr_import::metrics::report::timing::Timing::release_oracle(
            self.origin_ns,
            self.observed_ns,
        )
    }
}

impl Window {
    pub(super) const fn deadline_ns(self) -> u64 {
        self.deadline_ns
    }
}

impl DescriptiveBaseline {
    /// Transform the existing absolute owner/evidence envelope into descriptive-launch authority.
    pub(super) const fn transform(envelope: evidence::Envelope) -> Self {
        Self { envelope }
    }

    pub(super) const fn window(&self) -> Window {
        Window {
            deadline_ns: self.envelope.deadline_ns(),
        }
    }

    /// Transform one exact timestamp into descriptive evidence without applying release gates.
    pub(super) fn observe(&self, observed_ns: u64) -> Result<DescriptiveVisibility, Error> {
        let latency_ns = observed_ns
            .checked_sub(self.envelope.origin_ns())
            .ok_or(Error::Clock)?;
        if observed_ns > self.envelope.deadline_ns() {
            return Err(Error::VisibleAfterDeadline {
                mode: "descriptive baseline",
            });
        }
        Ok(DescriptiveVisibility {
            origin_ns: self.envelope.origin_ns(),
            observed_ns,
            latency_ns,
            exceeds_ten_seconds: latency_ns > RELEASE_MEDIAN_NS,
            exceeds_fifteen_seconds: latency_ns > RELEASE_VISIBLE_NS,
        })
    }
}

impl DescriptiveVisibility {
    pub(super) const fn origin_ns(&self) -> u64 {
        self.origin_ns
    }

    pub(super) const fn observed_ns(&self) -> u64 {
        self.observed_ns
    }

    pub(super) const fn latency_ns(&self) -> u64 {
        self.latency_ns
    }

    pub(super) const fn exceeds_ten_seconds(&self) -> bool {
        self.exceeds_ten_seconds
    }

    pub(super) const fn exceeds_fifteen_seconds(&self) -> bool {
        self.exceeds_fifteen_seconds
    }
}

impl ReleaseOracle {
    fn transform(origin_ns: u64) -> Result<Self, Error> {
        if origin_ns == 0 {
            return Err(Error::Clock);
        }
        let deadline_ns = origin_ns
            .checked_add(RELEASE_VISIBLE_NS)
            .ok_or(Error::Clock)?;
        Ok(Self {
            origin_ns,
            window: Window { deadline_ns },
        })
    }

    fn observe(&self, observed_ns: u64) -> Result<ReleaseVisibility, Error> {
        let latency_ns = observed_ns
            .checked_sub(self.origin_ns)
            .ok_or(Error::Clock)?;
        if observed_ns > self.window.deadline_ns {
            return Err(Error::VisibleAfterDeadline {
                mode: "release oracle",
            });
        }
        Ok(ReleaseVisibility {
            origin_ns: self.origin_ns,
            observed_ns,
            latency_ns,
        })
    }
}

impl ReleaseVisibility {
    pub(super) const fn origin_ns(&self) -> u64 {
        self.origin_ns
    }

    pub(super) const fn observed_ns(&self) -> u64 {
        self.observed_ns
    }

    pub(super) const fn latency_ns(&self) -> u64 {
        self.latency_ns
    }
}

impl ReleaseSeries {
    pub(super) fn transform(
        warmups: [ReleaseVisibility; 3],
        retained: [ReleaseVisibility; 5],
    ) -> Result<Self, Error> {
        let mut latencies = retained.each_ref().map(|visibility| visibility.latency_ns);
        latencies.sort_unstable();
        let median_ns = latencies[latencies.len() / 2];
        if median_ns > RELEASE_MEDIAN_NS {
            return Err(Error::ReleaseMedian);
        }
        Ok(Self {
            warmups,
            retained,
            median_ns,
        })
    }

    pub(super) const fn median_ns(&self) -> u64 {
        self.median_ns
    }

    pub(super) fn max_ns(&self) -> u64 {
        self.retained
            .iter()
            .map(|visibility| visibility.latency_ns)
            .max()
            .unwrap_or(0)
    }

    pub(super) fn observations(&self) -> impl Iterator<Item = &ReleaseVisibility> {
        self.warmups.iter().chain(self.retained.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Authority as _, DescriptiveBaseline, ReleaseOracle, ReleaseSeries, Visibility as _,
    };

    const ORIGIN_NS: u64 = 5_000_000_000;

    #[test]
    fn descriptive_visibility_after_release_limit_remains_truthful_evidence() {
        let envelope = rawr_import::metrics::evidence::Envelope::transform(ORIGIN_NS)
            .expect("owner envelope seals");
        let baseline = DescriptiveBaseline::transform(envelope);
        let visible = baseline
            .observe(ORIGIN_NS + 16_000_000_000)
            .expect("descriptive visibility before thirty minutes remains admissible");
        assert_eq!(visible.origin_ns(), ORIGIN_NS);
        assert_eq!(visible.observed_ns(), ORIGIN_NS + 16_000_000_000);
        assert_eq!(visible.latency_ns(), 16_000_000_000);
        assert!(visible.exceeds_ten_seconds());
        assert!(visible.exceeds_fifteen_seconds());
    }

    #[test]
    fn release_visibility_after_fifteen_seconds_rejects() {
        let release = ReleaseOracle::transform(ORIGIN_NS).expect("release authority seals");
        let visible = release
            .observe(ORIGIN_NS + 15_000_000_000)
            .expect("the exact inclusive release limit is admissible");
        assert_eq!(visible.origin_ns(), ORIGIN_NS);
        assert_eq!(visible.observed_ns(), ORIGIN_NS + 15_000_000_000);
        assert_eq!(visible.latency_ns(), 15_000_000_000);
        assert!(release.observe(ORIGIN_NS + 15_000_000_001).is_err());
    }

    #[test]
    fn timing_deadlines_share_the_exact_pre_spawn_origin() {
        let envelope = rawr_import::metrics::evidence::Envelope::transform(ORIGIN_NS)
            .expect("owner envelope seals");
        let baseline = DescriptiveBaseline::transform(envelope);
        let release = ReleaseOracle::transform(ORIGIN_NS).expect("release authority seals");
        assert_eq!(
            baseline.window().deadline_ns(),
            ORIGIN_NS + 1_800_000_000_000
        );
        assert_eq!(release.window.deadline_ns(), ORIGIN_NS + 15_000_000_000);
    }

    #[test]
    fn launch_authority_uses_one_fixed_release_deadline_and_emits_distinct_facts() {
        let envelope = rawr_import::metrics::evidence::Envelope::transform(ORIGIN_NS).unwrap();
        let release = ReleaseOracle::from_envelope(envelope).unwrap();
        let baseline = DescriptiveBaseline::from_envelope(envelope).unwrap();
        assert_eq!(release.window().deadline_ns(), ORIGIN_NS + 15_000_000_000);
        assert_eq!(
            baseline.window().deadline_ns(),
            ORIGIN_NS + 1_800_000_000_000
        );
        let release_visible = release.observe(ORIGIN_NS + 15_000_000_000).unwrap();
        let baseline_visible = baseline.observe(ORIGIN_NS + 15_000_000_000).unwrap();
        assert_ne!(
            release_visible.report().unwrap(),
            baseline_visible.report().unwrap()
        );
        assert!(matches!(
            release.observe(ORIGIN_NS - 1),
            Err(super::Error::Clock)
        ));
        assert!(matches!(
            release.observe(ORIGIN_NS + 15_000_000_001),
            Err(super::Error::VisibleAfterDeadline {
                mode: "release oracle"
            })
        ));
        // Receiving/authenticating the runner later cannot restart this authority.
        assert_eq!(release.window().deadline_ns(), ORIGIN_NS + 15_000_000_000);
    }

    #[test]
    fn release_series_accepts_only_release_visibility_proofs() {
        let visibility = |latency| {
            ReleaseOracle::transform(ORIGIN_NS)
                .expect("release authority seals")
                .observe(ORIGIN_NS + latency)
                .expect("release visibility seals")
        };
        let series = ReleaseSeries::transform(
            [
                visibility(9_000_000_000),
                visibility(9_000_000_000),
                visibility(9_000_000_000),
            ],
            [
                visibility(8_000_000_000),
                visibility(9_000_000_000),
                visibility(10_000_000_000),
                visibility(10_000_000_000),
                visibility(11_000_000_000),
            ],
        )
        .expect("five retained release proofs meet the median gate");
        assert_eq!(series.median_ns, 10_000_000_000);
        assert_eq!(series.max_ns(), 11_000_000_000);
        assert_eq!(series.observations().count(), 8);

        let over_median = ReleaseSeries::transform(
            [
                visibility(9_000_000_000),
                visibility(9_000_000_000),
                visibility(9_000_000_000),
            ],
            [
                visibility(9_000_000_000),
                visibility(10_000_000_000),
                visibility(10_000_000_001),
                visibility(11_000_000_000),
                visibility(12_000_000_000),
            ],
        );
        assert!(over_median.is_err());
    }
}
