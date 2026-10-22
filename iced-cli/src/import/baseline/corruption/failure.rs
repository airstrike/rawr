//! Preserve the initiating refusal and already-received canonical capsule.

use rawr_import::metrics::report;

#[derive(serde::Serialize)]
pub(super) struct Record<'a> {
    schema: &'static str,
    original: Original<'a>,
    reached: &'a super::Reached,
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum Original<'a> {
    Owner {
        owner: report::Command,
        reason: &'a str,
    },
    Bound {
        field: &'static str,
        limit: u64,
    },
    OwnerObserved {
        source: Box<Original<'a>>,
        canonical_capsule: &'a str,
    },
    Retirement {
        source: Box<Original<'a>>,
        cleanup: Box<Original<'a>>,
    },
    Other {
        diagnostic: String,
        causes: Vec<String>,
    },
}

/// Use the same recursive owner representation without inventing corruption endpoints.
pub(super) fn owner(error: &super::super::Error) -> Result<Box<[u8]>, super::Error> {
    #[derive(serde::Serialize)]
    struct Owner<'a> {
        schema: &'static str,
        original: Original<'a>,
    }
    super::encode(&Owner {
        schema: "rawr.phase9.cell.owner-failure/v1",
        original: Original::owner(error),
    })
}

impl<'a> Record<'a> {
    pub(super) fn capture(error: &'a super::Error, reached: &'a super::Reached) -> Self {
        let original = match error {
            super::Error::Owner(error) => Original::owner(error),
            super::Error::Reached { source, .. } => Self::capture(source, reached).original,
            error => Original::other(error),
        };
        Self {
            schema: "rawr.phase9.corruption.failure/v2",
            original,
            reached,
        }
    }
}

impl<'a> Original<'a> {
    fn owner(error: &'a super::super::Error) -> Self {
        match error {
            super::super::Error::Owner { command, reason } => Self::Owner {
                owner: *command,
                reason,
            },
            super::super::Error::Bound { field, limit } => Self::Bound {
                field,
                limit: *limit,
            },
            super::super::Error::OwnerObserved { source, evidence } => Self::OwnerObserved {
                source: Box::new(Self::owner(source)),
                canonical_capsule: evidence,
            },
            super::super::Error::CellRetirement { source, cleanup } => Self::Retirement {
                source: Box::new(Self::owner(source)),
                cleanup: Box::new(Self::owner(cleanup)),
            },
            error => Self::other(error),
        }
    }

    fn other(error: &dyn std::error::Error) -> Self {
        let mut causes = Vec::new();
        let mut source = error.source();
        while let Some(error) = source {
            causes.push(error.to_string());
            source = error.source();
        }
        Self::Other {
            diagnostic: error.to_string(),
            causes,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn already_received_capsule_survives_failure_before_launch_assignment() {
        let canonical = "{\"outcome\":{\"failure\":\"first refusal\"},\"counter\":17}\n";
        let error = super::super::Error::Owner(super::super::super::Error::CellRetirement {
            source: Box::new(super::super::super::Error::OwnerObserved {
                source: Box::new(super::super::super::Error::Owner {
                    command: rawr_import::metrics::report::Command::Check,
                    reason: "original resource refusal".into(),
                }),
                evidence: canonical.into(),
            }),
            cleanup: Box::new(super::super::super::Error::Process {
                reason: "later retirement failure".into(),
            }),
        });
        let reached = super::super::Reached::default();
        let record = super::Record::capture(&error, &reached);
        let value: serde_json::Value =
            serde_json::from_slice(&super::super::encode(&record).unwrap()).unwrap();
        assert!(value["reached"]["setup"].is_null());
        assert!(value["reached"]["retained"].is_null());
        assert_eq!(value["original"]["kind"], "retirement");
        assert_eq!(value["original"]["source"]["canonical_capsule"], canonical);
        assert_eq!(
            value["original"]["source"]["source"]["reason"],
            "original resource refusal"
        );
        assert_eq!(value["original"]["source"]["source"]["owner"], "check");
    }
}
