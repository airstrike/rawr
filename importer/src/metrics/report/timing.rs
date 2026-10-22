//! Inert canonical timing and retirement evidence.

const TEN_SECONDS_NS: u64 = 10_000_000_000;
const FIFTEEN_SECONDS_NS: u64 = 15_000_000_000;
const THIRTY_MINUTES_NS: u64 = 1_800_000_000_000;

/// Canonical timing facts. Deserialization grants no observation authority.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Timing {
    #[serde(flatten)]
    state: State,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
enum State {
    DescriptiveBaseline {
        origin_ns: u64,
        observed_ns: u64,
        latency_ns: u64,
        exceeds_ten_seconds: bool,
        exceeds_fifteen_seconds: bool,
    },
    ReleaseOracle {
        origin_ns: u64,
        observed_ns: u64,
        latency_ns: u64,
        exceeds_ten_seconds: bool,
        exceeds_fifteen_seconds: bool,
    },
    NotApplicable {
        reason: Box<str>,
    },
    Unavailable {
        reason: Box<str>,
    },
}

/// Canonical post-press lifecycle facts.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Retirement {
    #[serde(flatten)]
    state: RetirementState,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum RetirementState {
    Completed {
        pressed_ns: u64,
        completed_ns: u64,
        latency_ns: u64,
        ax_press_count: u8,
        window_disappeared: bool,
        final_capsule_and_eof: bool,
        normal_process_return: bool,
    },
    NotApplicable {
        reason: Box<str>,
    },
    Unavailable {
        reason: Box<str>,
    },
}

impl Timing {
    /// Transform exact timestamps into descriptive comparison facts.
    pub fn descriptive_baseline(origin_ns: u64, observed_ns: u64) -> Result<Self, super::Error> {
        let latency_ns = observed_ns
            .checked_sub(origin_ns)
            .ok_or(super::Error::TimingClock)?;
        if origin_ns == 0 || latency_ns > THIRTY_MINUTES_NS {
            return Err(super::Error::DescriptiveVisible);
        }
        Ok(Self {
            state: State::DescriptiveBaseline {
                origin_ns,
                observed_ns,
                latency_ns,
                exceeds_ten_seconds: latency_ns > TEN_SECONDS_NS,
                exceeds_fifteen_seconds: latency_ns > FIFTEEN_SECONDS_NS,
            },
        })
    }

    /// Retain why a window observation has no meaning for this owner.
    pub fn not_applicable(reason: impl Into<Box<str>>) -> Self {
        Self {
            state: State::NotApplicable {
                reason: reason.into(),
            },
        }
    }

    /// Retain a concrete reason why timing evidence was unavailable.
    pub fn unavailable(reason: impl Into<Box<str>>) -> Self {
        Self {
            state: State::Unavailable {
                reason: reason.into(),
            },
        }
    }

    pub(super) fn validate(&self) -> Result<(), super::Error> {
        match &self.state {
            State::DescriptiveBaseline {
                origin_ns,
                observed_ns,
                latency_ns,
                exceeds_ten_seconds,
                exceeds_fifteen_seconds,
            } => validate_visible(
                *origin_ns,
                *observed_ns,
                *latency_ns,
                *exceeds_ten_seconds,
                *exceeds_fifteen_seconds,
                false,
            ),
            State::ReleaseOracle {
                origin_ns,
                observed_ns,
                latency_ns,
                exceeds_ten_seconds,
                exceeds_fifteen_seconds,
            } => validate_visible(
                *origin_ns,
                *observed_ns,
                *latency_ns,
                *exceeds_ten_seconds,
                *exceeds_fifteen_seconds,
                true,
            ),
            State::NotApplicable { reason } | State::Unavailable { reason } => {
                validate_reason(reason)
            }
        }
    }

    pub(super) fn is_descriptive_baseline(&self) -> bool {
        matches!(self.state, State::DescriptiveBaseline { .. })
    }

    pub(super) fn reason(&self) -> Option<&str> {
        match &self.state {
            State::NotApplicable { reason } | State::Unavailable { reason } => Some(reason),
            State::DescriptiveBaseline { .. } | State::ReleaseOracle { .. } => None,
        }
    }

    /// Record inert release timestamps; this grants no live release authority.
    pub fn release_oracle(origin_ns: u64, observed_ns: u64) -> Result<Self, super::Error> {
        let latency_ns = observed_ns
            .checked_sub(origin_ns)
            .ok_or(super::Error::TimingClock)?;
        if origin_ns == 0 || latency_ns > FIFTEEN_SECONDS_NS {
            return Err(super::Error::ReleaseVisible);
        }
        Ok(Self {
            state: State::ReleaseOracle {
                origin_ns,
                observed_ns,
                latency_ns,
                exceeds_ten_seconds: latency_ns > TEN_SECONDS_NS,
                exceeds_fifteen_seconds: false,
            },
        })
    }
}

impl Retirement {
    /// Transform one completed one-press retirement into canonical facts.
    pub fn completed(pressed_ns: u64, completed_ns: u64) -> Result<Self, super::Error> {
        let latency_ns = completed_ns
            .checked_sub(pressed_ns)
            .ok_or(super::Error::TimingClock)?;
        if latency_ns > TEN_SECONDS_NS {
            return Err(super::Error::RetirementDeadline);
        }
        Ok(Self {
            state: RetirementState::Completed {
                pressed_ns,
                completed_ns,
                latency_ns,
                ax_press_count: 1,
                window_disappeared: true,
                final_capsule_and_eof: true,
                normal_process_return: true,
            },
        })
    }

    /// Retain why retirement has no meaning for this owner.
    pub fn not_applicable(reason: impl Into<Box<str>>) -> Self {
        Self {
            state: RetirementState::NotApplicable {
                reason: reason.into(),
            },
        }
    }

    /// Retain a concrete reason why retirement evidence was unavailable.
    pub fn unavailable(reason: impl Into<Box<str>>) -> Self {
        Self {
            state: RetirementState::Unavailable {
                reason: reason.into(),
            },
        }
    }

    pub(super) fn validate(&self) -> Result<(), super::Error> {
        match &self.state {
            RetirementState::Completed {
                pressed_ns,
                completed_ns,
                latency_ns,
                ax_press_count,
                window_disappeared,
                final_capsule_and_eof,
                normal_process_return,
            } => {
                if completed_ns.checked_sub(*pressed_ns) != Some(*latency_ns)
                    || *latency_ns > TEN_SECONDS_NS
                    || *ax_press_count != 1
                    || !window_disappeared
                    || !final_capsule_and_eof
                    || !normal_process_return
                {
                    return Err(super::Error::RetirementEvidence);
                }
                Ok(())
            }
            RetirementState::NotApplicable { reason } | RetirementState::Unavailable { reason } => {
                validate_reason(reason)
            }
        }
    }

    pub(super) fn is_completed(&self) -> bool {
        matches!(self.state, RetirementState::Completed { .. })
    }

    pub(super) fn reason(&self) -> Option<&str> {
        match &self.state {
            RetirementState::NotApplicable { reason } | RetirementState::Unavailable { reason } => {
                Some(reason)
            }
            RetirementState::Completed { .. } => None,
        }
    }
}

fn validate_visible(
    origin_ns: u64,
    observed_ns: u64,
    latency_ns: u64,
    exceeds_ten_seconds: bool,
    exceeds_fifteen_seconds: bool,
    release: bool,
) -> Result<(), super::Error> {
    if origin_ns == 0
        || observed_ns.checked_sub(origin_ns) != Some(latency_ns)
        || exceeds_ten_seconds != (latency_ns > TEN_SECONDS_NS)
        || exceeds_fifteen_seconds != (latency_ns > FIFTEEN_SECONDS_NS)
    {
        return Err(super::Error::TimingEvidence);
    }
    if release && latency_ns > FIFTEEN_SECONDS_NS {
        return Err(super::Error::ReleaseVisible);
    }
    if !release && latency_ns > THIRTY_MINUTES_NS {
        return Err(super::Error::DescriptiveVisible);
    }
    Ok(())
}

fn validate_reason(reason: &str) -> Result<(), super::Error> {
    if reason.is_empty() {
        Err(super::Error::Empty {
            field: "timing.reason",
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn descriptive_schema_cannot_pass_as_release_state() {
        let baseline = super::Timing::descriptive_baseline(1, 16_000_000_001)
            .expect("descriptive report seals");
        let bytes = serde_json::to_vec(&baseline).expect("descriptive timing encodes");
        let text = std::str::from_utf8(&bytes).expect("JSON is UTF-8");
        assert!(text.contains("descriptive-baseline"));
        assert!(!text.contains("release-oracle"));

        assert!(super::Timing::release_oracle(1, 16_000_000_002).is_err());
    }
}
