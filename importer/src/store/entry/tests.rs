use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::node::{self, Error, GraphBudget};

#[cfg(feature = "import-baseline")]
mod publication_history {
    use std::fs;

    use crate::metrics::{publication, recorder};
    use crate::node::{Digest, Error};
    use crate::store::{control, entry, lease, recovery, Store};

    #[test]
    fn publication_history_records_real_winner_and_loser_directory_cleanup() {
        let fixture = super::Fixture::new();
        let _store = Store::open(&fixture.0, [1; 32]).unwrap();
        let target = fixture.0.join("published");
        let recorder = recorder::Recorder::transform(100).unwrap();
        let scope = recorder.scope().unwrap();
        for expected in [true, false] {
            let (result, retained) = entry::publish_sealed_directory(
                &fixture.0,
                &target,
                "payload",
                b"winner",
                publication::Class::Content(crate::metrics::NodeKind::Toolchain),
                None,
                |_| Ok((Box::from(&b"record"[..]), 7)),
            )
            .unwrap();
            assert_eq!(matches!(result, entry::Publication::Published), expected);
            assert_eq!(retained, expected.then_some(7));
        }
        lease::with_publication(&fixture.0, _store.graph_budget(), |authority| {
            entry::publish_directory(
                authority,
                &fixture.0,
                &target,
                "payload",
                b"ignored",
                publication::Class::Index(crate::metrics::NodeKind::Toolchain),
                None,
                None,
            )
        })
        .unwrap();
        drop(scope);
        let evidence = recorder
            .snapshot()
            .unwrap()
            .accounting
            .publications
            .unwrap();
        assert!(evidence.closed());
        let row = evidence.nodes[0].content;
        assert_eq!(
            (row.started, row.created, row.renamed, row.lost, row.cleaned),
            (2, 2, 1, 1, 1)
        );
        assert_eq!(
            (
                evidence.nodes[0].index.existing,
                evidence.nodes[0].index.created
            ),
            (1, 0)
        );
        assert_eq!(fs::read(target.join("payload")).unwrap(), b"winner");
        assert_eq!(
            fs::read_dir(fixture.0.join("temporaries")).unwrap().count(),
            0
        );
    }

    #[test]
    fn publication_history_observes_rename_before_parent_sync_failure() {
        use crate::metrics::{cell::project, principal};
        use principal::temporary;
        use std::os::unix::ffi::OsStrExt as _;
        let fixture = super::Fixture::new();
        fs::create_dir(fixture.0.join("temporaries")).unwrap();
        let target = fixture.0.join(crate::store::action_relative(
            crate::node::Kind::Api,
            crate::node::ActionKey::from_bytes([3; 32]),
        ));
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let before = project::Manifest::read(&fixture.0).unwrap();
        let recorder = recorder::Recorder::transform(100).unwrap();
        let scope = recorder.scope().unwrap();
        let budget = crate::node::GraphBudget::new();
        let principal = crate::metrics::principal::Attempt::start(
            crate::metrics::principal::Identity::observe(
                &fixture.0,
                &target,
                crate::node::Kind::Api,
                1,
                crate::node::ActionKey::from_bytes([3; 32]),
                Digest::from_bytes([4; 32]),
                crate::metrics::principal::Target::Envelope {
                    content: [5; 32],
                    lease: crate::metrics::principal::Lease::Action([3; 32]),
                },
                &budget,
            ),
            &budget,
        );
        let result = entry::publish_sealed_with(
            &fixture.0,
            &target,
            "envelope",
            b"value",
            publication::Class::Action(crate::metrics::NodeKind::Api),
            Some(&principal),
            |_| {
                principal.sealed(
                    crate::metrics::principal::Commitment::produced(b"value"),
                    Some(b"record"),
                );
                Ok((Box::from(&b"record"[..]), ()))
            },
            |path| {
                if path == target.parent().unwrap() {
                    Err(Error::State("parent sync failed after rename"))
                } else {
                    entry::sync_directory(path)
                }
            },
        );
        assert!(matches!(
            result,
            Err(Error::State("parent sync failed after rename"))
        ));
        drop(principal);
        drop(scope);
        let mut snapshot = recorder.snapshot().unwrap();
        snapshot
            .principal
            .as_mut()
            .unwrap()
            .prepare_cell_transport(&mut snapshot.accounting, 100)
            .unwrap();
        let decoded: principal::Evidence = serde_json::from_slice(
            &serde_json::to_vec(snapshot.principal.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        let events: Vec<_> = decoded
            .events
            .as_slice()
            .iter()
            .filter(|event| !matches!(event, principal::Event::Temporary { .. }))
            .collect();
        assert!(matches!(
            events.as_slice(),
            [
                crate::metrics::principal::Event::Started { .. },
                crate::metrics::principal::Event::Sealed { .. },
                crate::metrics::principal::Event::Renamed { .. },
                crate::metrics::principal::Event::Finished {
                    outcome: crate::metrics::principal::Outcome::Failed,
                    ..
                }
            ]
        ));
        let after = project::Manifest::read(&fixture.0).unwrap();
        let other = principal::Evidence {
            cell_transport: true,
            ..principal::Evidence::default()
        };
        let recoveries = principal::recovery::join::Joined {
            batches: Vec::new(),
            temporaries: Vec::new(),
            modes: Vec::new(),
            missing: Vec::new(),
            retained: Vec::new(),
        };
        let pid = std::process::id();
        let joined = temporary::join::pair(
            [(pid, &decoded), (pid + 1, &other)],
            &fixture.0,
            &before,
            &after,
            &recoveries,
        )
        .unwrap();
        assert_eq!(joined.attempts.len(), 1);
        let attempt = &joined.attempts[0];
        assert_eq!(attempt.outcome, Some(temporary::Outcome::Failed));
        assert!(attempt
            .steps
            .iter()
            .any(|step| step.action == temporary::Action::Rename && step.succeeded));
        assert!(attempt
            .steps
            .iter()
            .any(|step| step.action == temporary::Action::SyncParent && !step.succeeded));
        assert_eq!(attempt.cleanup_exists, Some(false));
        assert!(temporary::join::pair(
            [(pid + 2, &decoded), (pid + 1, &other)],
            &fixture.0,
            &before,
            &after,
            &recoveries
        )
        .is_err());
        let mut wire = serde_json::to_value(&decoded).unwrap();
        let rows = wire["events"].as_array_mut().unwrap();
        let seal = rows
            .iter()
            .position(|row| row["event"] == "sealed")
            .unwrap();
        let row = rows.remove(seal);
        let reserved = rows
            .iter()
            .position(|row| row["event"] == "temporary" && row["change"]["lifecycle"] == "reserved")
            .unwrap();
        rows.insert(reserved + 1, row);
        let reordered: principal::Evidence = serde_json::from_value(wire).unwrap();
        assert!(
            temporary::join::pair(
                [(pid, &reordered), (pid + 1, &other)],
                &fixture.0,
                &before,
                &after,
                &recoveries
            )
            .is_err(),
            "a seal cannot precede the source payload write and sync"
        );
        // Derive a failed temporary-directory sync with a false exists result
        // from the real writes above. Both completed members remain observed;
        // no cleanup occurrence can account for either member disappearing.
        let mut rows = decoded.events.as_slice().to_vec();
        let rename = rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    principal::Event::Temporary {
                        change: temporary::Event::Step {
                            action: temporary::Action::Rename,
                            ..
                        }
                    }
                )
            })
            .unwrap();
        let operation = rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    principal::Event::Temporary {
                        change: temporary::Event::Operation { .. }
                    }
                )
            })
            .unwrap();
        rows.drain(rename..operation);
        let sync = rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    principal::Event::Temporary {
                        change: temporary::Event::Step {
                            action: temporary::Action::SyncTemporary,
                            ..
                        }
                    }
                )
            })
            .unwrap();
        if let principal::Event::Temporary {
            change: temporary::Event::Result { succeeded, .. },
        } = &mut rows[sync + 1]
        {
            *succeeded = false;
        } else {
            unreachable!();
        }
        let mut wire = serde_json::to_value(&decoded).unwrap();
        wire["events"] = serde_json::to_value(&rows).unwrap();
        let failed_sync: principal::Evidence = serde_json::from_value(wire).unwrap();
        let prefix = attempt.temporary.unwrap().relative();
        let original = target.strip_prefix(&fixture.0).unwrap();
        let mut retained = after.clone();
        for row in retained.members.iter_mut() {
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&row.path));
            if let Ok(suffix) = path.strip_prefix(original) {
                row.path = prefix.join(suffix).as_os_str().as_encoded_bytes().into();
            }
        }
        for row in retained.directories.iter_mut() {
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&row.path));
            if let Ok(suffix) = path.strip_prefix(original) {
                row.path = if suffix.as_os_str().is_empty() {
                    prefix.clone()
                } else {
                    prefix.join(suffix)
                }
                .as_os_str()
                .as_encoded_bytes()
                .into();
            }
        }
        retained
            .members
            .sort_by(|left, right| left.path.cmp(&right.path));
        retained
            .directories
            .sort_by(|left, right| left.path.cmp(&right.path));
        temporary::join::pair(
            [(pid, &failed_sync), (pid + 1, &other)],
            &fixture.0,
            &before,
            &retained,
            &recoveries,
        )
        .unwrap();
        for name in ["envelope", "allocation"] {
            let path = prefix.join(name);
            let mut missing = retained.clone();
            missing.members = missing
                .members
                .iter()
                .filter(|row| row.path.as_ref() != path.as_os_str().as_encoded_bytes())
                .cloned()
                .collect();
            assert!(
                temporary::join::pair(
                    [(pid, &failed_sync), (pid + 1, &other)],
                    &fixture.0,
                    &before,
                    &missing,
                    &recoveries
                )
                .is_err(),
                "each exclusively opened member must remain or have a removal occurrence"
            );
        }
        let mut corrupt = retained.clone();
        let payload = prefix.join("envelope");
        corrupt
            .members
            .iter_mut()
            .find(|row| row.path.as_ref() == payload.as_os_str().as_encoded_bytes())
            .unwrap()
            .digest[0] ^= 1;
        assert!(
            temporary::join::pair(
                [(pid, &failed_sync), (pid + 1, &other)],
                &fixture.0,
                &before,
                &corrupt,
                &recoveries
            )
            .is_err(),
            "completed temporary bytes remain bound before rename"
        );
        let mut rows = decoded.events.as_slice().to_vec();
        let allocation = rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    principal::Event::Temporary {
                        change: temporary::Event::Step {
                            action: temporary::Action::Open(temporary::Member::Allocation),
                            ..
                        }
                    }
                )
            })
            .unwrap();
        let operation = rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    principal::Event::Temporary {
                        change: temporary::Event::Operation { .. }
                    }
                )
            })
            .unwrap();
        rows.drain(allocation..operation);
        let mut no_allocation = retained.clone();
        let path = prefix.join("allocation");
        no_allocation.members = no_allocation
            .members
            .iter()
            .filter(|row| row.path.as_ref() != path.as_os_str().as_encoded_bytes())
            .cloned()
            .collect();
        let mut wire = serde_json::to_value(&decoded).unwrap();
        wire["events"] = serde_json::to_value(&rows).unwrap();
        let skipped: principal::Evidence = serde_json::from_value(wire.clone()).unwrap();
        assert!(
            temporary::join::pair(
                [(pid, &skipped), (pid + 1, &other)],
                &fixture.0,
                &before,
                &no_allocation,
                &recoveries
            )
            .is_err(),
            "a completed source seal proceeds to allocation open"
        );
        rows.retain(|row| !matches!(row, principal::Event::Sealed { .. }));
        wire["events"] = serde_json::to_value(rows).unwrap();
        let seal_failed: principal::Evidence = serde_json::from_value(wire).unwrap();
        let unsealed = temporary::join::pair(
            [(pid, &seal_failed), (pid + 1, &other)],
            &fixture.0,
            &before,
            &no_allocation,
            &recoveries,
        )
        .unwrap();
        assert!(
            unsealed
                .endpoints
                .iter()
                .any(|endpoint| !endpoint.unavailable_commitments.is_empty()),
            "early callback failure does not invent the payload commitment"
        );
        // Derive a later action retirement from the real completed member
        // writes. Its validated allocation/member facts bind the moved bytes;
        // a failed recursive removal may retain either member.
        let retirement_token = u64::MAX - 1;
        let moved = principal::retirement::Temporary {
            process: pid,
            sequence: u64::MAX - 2,
        };
        let mut rows = decoded.events.as_slice().to_vec();
        let target_kind = principal::retirement::Target::Action {
            kind: crate::node::Kind::Api as u8,
            storage: 2,
            action: [3; 32],
        };
        let facts = principal::retirement::Facts {
            allocation: Some(principal::pin::Reference {
                class: 1,
                kind: crate::node::Kind::Api as u8,
                schema: 1,
                key: [3; 32],
                bytes: 4096,
                allocations: 1,
                members: 3,
                record: principal::Commitment::produced(b"record").sha256,
            }),
            owner: Some(principal::retirement::Owner {
                lease: principal::Lease::Action([3; 32]),
                publisher: [4; 32],
            }),
            member: Some(principal::Commitment::produced(b"value")),
        };
        for change in [
            principal::retirement::Event::Started {
                attempt: retirement_token,
                publication: 1,
                root: principal::path_digest(&fixture.0),
                relative: principal::path_digest(&target_kind.relative().unwrap()),
                target: target_kind,
                cause: principal::retirement::Cause::ActionEviction,
                facts,
            },
            principal::retirement::Event::Relocated {
                attempt: retirement_token,
                temporary: moved,
                relative: principal::path_digest(&moved.relative()),
            },
            principal::retirement::Event::RemovalStarted {
                attempt: retirement_token,
            },
            principal::retirement::Event::RemovalFinished {
                attempt: retirement_token,
                succeeded: false,
            },
            principal::retirement::Event::Finished {
                attempt: retirement_token,
                succeeded: false,
            },
        ] {
            rows.push(principal::Event::Retirement { change });
        }
        let mut wire = serde_json::to_value(&decoded).unwrap();
        wire["events"] = serde_json::to_value(&rows).unwrap();
        let relocated: principal::Evidence = serde_json::from_value(wire).unwrap();
        let mut moved_after = after.clone();
        for row in moved_after.members.iter_mut() {
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&row.path));
            if let Ok(suffix) = path.strip_prefix(original) {
                row.path = moved
                    .relative()
                    .join(suffix)
                    .as_os_str()
                    .as_encoded_bytes()
                    .into();
            }
        }
        for row in moved_after.directories.iter_mut() {
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&row.path));
            if path == original {
                row.path = moved.relative().as_os_str().as_encoded_bytes().into();
            }
        }
        moved_after
            .members
            .sort_by(|left, right| left.path.cmp(&right.path));
        moved_after
            .directories
            .sort_by(|left, right| left.path.cmp(&right.path));
        let processes = [(pid, &relocated), (pid + 1, &other)];
        let mut retirements =
            principal::retirement::join::pair(processes, &fixture.0, &before, &moved_after)
                .unwrap();
        let pins =
            principal::pin::join::pair(processes, &fixture.0, &before, &moved_after).unwrap();
        let mut composed =
            temporary::join::pair(processes, &fixture.0, &before, &moved_after, &recoveries)
                .unwrap();
        composed
            .compose(&before, &moved_after, &retirements, &pins, &recoveries)
            .unwrap();
        assert!(composed.published.iter().any(|endpoint| !endpoint.directory
            && endpoint.present
            && endpoint.unavailable_commitments.is_empty()));
        let mut damaged = moved_after.clone();
        damaged
            .members
            .iter_mut()
            .find(|row| row.path.ends_with(b"/envelope"))
            .unwrap()
            .digest[0] ^= 1;
        assert!(
            composed
                .compose(&before, &damaged, &retirements, &pins, &recoveries)
                .is_err(),
            "surviving relocated bytes retain their matched immutable commitment"
        );
        retirements.attempts[0].facts.member = None;
        composed
            .compose(&before, &moved_after, &retirements, &pins, &recoveries)
            .unwrap();
        assert!(
            composed
                .compose(&before, &damaged, &retirements, &pins, &recoveries)
                .is_err(),
            "the same allocation bytes retain their already-bound payload commitment"
        );
        retirements.attempts[0].facts.member =
            Some(principal::Commitment::produced(b"contradiction"));
        assert!(
            composed
                .compose(&before, &moved_after, &retirements, &pins, &recoveries)
                .is_err(),
            "a matching record cannot contradict its bound payload"
        );
        let evidence = recorder
            .snapshot()
            .unwrap()
            .accounting
            .publications
            .unwrap();
        let row = evidence.nodes[2].action;
        assert_eq!(
            (
                row.created,
                row.renamed,
                row.operation_failed,
                row.unresolved
            ),
            (1, 1, 1, 0)
        );
        assert!(!evidence.closed());
        assert!(target.exists());
        assert_eq!(
            fs::read_dir(fixture.0.join("temporaries")).unwrap().count(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_history_keeps_operation_and_cleanup_failures_with_existing_precedence() {
        let fixture = super::Fixture::new();
        let temporaries = fixture.0.join("temporaries");
        fs::create_dir(&temporaries).unwrap();
        let recorder = recorder::Recorder::transform(100).unwrap();
        let scope = recorder.scope().unwrap();
        let result = entry::publish_sealed_directory::<()>(
            &fixture.0,
            &fixture.0.join("published"),
            "payload",
            b"value",
            publication::Class::Content(crate::metrics::NodeKind::Api),
            None,
            |_| {
                let temporary = fs::read_dir(&temporaries)
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path();
                fs::remove_dir_all(&temporary).unwrap();
                fs::write(&temporary, b"force cleanup type failure").unwrap();
                Err(Error::State("original seal failure"))
            },
        );
        assert!(
            matches!(
                result,
                Err(Error::Io {
                    operation: "retire sealed store publication reservation",
                    ..
                })
            ),
            "existing cleanup error precedence is preserved"
        );
        drop(scope);
        let row = recorder
            .snapshot()
            .unwrap()
            .accounting
            .publications
            .unwrap()
            .nodes[2]
            .content;
        assert_eq!(
            (
                row.operation_failed,
                row.cleanup_failed,
                row.unresolved,
                row.cleaned
            ),
            (1, 1, 1, 0)
        );
    }

    #[test]
    fn ledger_replacement_retains_rename_before_sync_failure() {
        use crate::metrics::{cell::project, principal};
        use principal::{ledger, temporary};
        let fixture = super::Fixture::new();
        let store = Store::open(&fixture.0, [1; 32]).unwrap();
        let target = store.root().join("ledgers/installation");
        let before = fs::read(&target).unwrap();
        let initial = project::Manifest::read(store.root()).unwrap();
        let recorder = recorder::Recorder::transform(1_000).unwrap();
        let scope = recorder.scope().unwrap();
        let result = lease::with_publication(store.root(), store.graph_budget(), |authority| {
            let admitted = control::Admitted::seal(
                authority,
                store.root(),
                control::Plan::replacement(&target, before.len()),
            )?;
            entry::replace_admitted_file_with(
                admitted,
                authority,
                store.root(),
                &target,
                &before,
                publication::Replacement::InstallationSave,
                Some(
                    ledger::Write::installation(ledger::Cause::Eviction)
                        .with_before(ledger::Before::Bytes(&before)),
                ),
                |_| Err(Error::State("ledger parent sync failed after rename")),
            )
        });
        assert!(matches!(
            result,
            Err(Error::State("ledger parent sync failed after rename"))
        ));
        assert_eq!(fs::read(&target).unwrap(), before);
        drop(scope);
        let mut snapshot = recorder.snapshot().unwrap();
        snapshot
            .principal
            .as_mut()
            .unwrap()
            .prepare_cell_transport(&mut snapshot.accounting, 1_000)
            .unwrap();
        let evidence: principal::Evidence = serde_json::from_slice(
            &serde_json::to_vec(snapshot.principal.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(evidence.fault.is_none());
        let rows: Vec<_> = evidence
            .events
            .as_slice()
            .iter()
            .filter_map(|event| match event {
                principal::Event::Ledger { change } => Some(change),
                _ => None,
            })
            .collect();
        assert!(matches!(
            rows.as_slice(),
            [
                ledger::Event::Started { .. },
                ledger::Event::Renamed { .. },
                ledger::Event::Finished {
                    succeeded: false,
                    ..
                }
            ]
        ));
        let after = project::Manifest::read(store.root()).unwrap();
        let other = principal::Evidence {
            cell_transport: true,
            ..principal::Evidence::default()
        };
        let recoveries = principal::recovery::join::Joined {
            batches: Vec::new(),
            temporaries: Vec::new(),
            modes: Vec::new(),
            missing: Vec::new(),
            retained: Vec::new(),
        };
        let pid = std::process::id();
        ledger::join::pair(
            [(pid, &evidence), (pid + 1, &other)],
            store.root(),
            &initial,
            &after,
        )
        .unwrap();
        let joined = temporary::join::pair(
            [(pid, &evidence), (pid + 1, &other)],
            store.root(),
            &initial,
            &after,
            &recoveries,
        )
        .unwrap();
        assert_eq!(joined.attempts.len(), 1);
        assert_eq!(joined.attempts[0].form, temporary::Form::Replacement);
        assert_eq!(
            joined.attempts[0].commitments[0],
            Some(principal::Commitment::produced(&before))
        );
        assert_eq!(joined.attempts[0].outcome, Some(temporary::Outcome::Failed));
        let mut wire = serde_json::to_value(&evidence).unwrap();
        let rows = wire["events"].as_array_mut().unwrap();
        let sync = rows
            .iter()
            .position(|row| {
                row["event"] == "temporary"
                    && row["change"]["lifecycle"] == "step"
                    && row["change"]["action"] == "sync-parent"
            })
            .unwrap();
        rows[sync + 1]["change"]["succeeded"] = serde_json::json!(true);
        let altered: principal::Evidence = serde_json::from_value(wire).unwrap();
        // No fallible operation remains after the final successful parent sync.
        assert!(temporary::join::pair(
            [(pid, &altered), (pid + 1, &other)],
            store.root(),
            &initial,
            &after,
            &recoveries
        )
        .is_err());
    }

    #[test]
    fn publication_history_observes_actual_file_replacement_and_enrollment_retirement() {
        let fixture = super::Fixture::new();
        let store = Store::open(&fixture.0, [1; 32]).unwrap();
        let target = store.root().join("ledgers/projects").join("ab".repeat(32));
        fs::write(&target, b"before").unwrap();
        let recorder = recorder::Recorder::transform(100).unwrap();
        let scope = recorder.scope().unwrap();
        lease::with_publication(store.root(), store.graph_budget(), |authority| {
            let admitted = control::Admitted::seal(
                authority,
                store.root(),
                control::Plan::replacement(&target, 5),
            )?;
            entry::replace_admitted_file(
                admitted,
                authority,
                store.root(),
                &target,
                b"after",
                publication::Replacement::PinBind,
                None,
            )
        })
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"after");
        // Remove the deliberately inert replacement fixture before normal accounting resumes.
        fs::remove_file(&target).unwrap();
        lease::enroll_project(
            store.root(),
            store.graph_budget(),
            Digest::from_bytes([2; 32]),
            &mut recovery::Budget::new(),
        )
        .unwrap();
        drop(scope);
        let evidence = recorder
            .snapshot()
            .unwrap()
            .accounting
            .publications
            .unwrap();
        assert!(evidence.closed());
        assert!(evidence
            .replacements
            .iter()
            .find(|row| row.purpose == publication::Replacement::PinBind)
            .unwrap()
            .counts
            .winners(1));
        let retired = evidence
            .retirements
            .iter()
            .find(|row| row.purpose == publication::Retirement::Enrollment)
            .unwrap()
            .counts;
        assert_eq!(
            (retired.created, retired.cleaned, retired.returned),
            (1, 1, 1)
        );
        assert!(retired.retired(false));
    }

    #[test]
    fn publication_history_abandoned_staged_retirement_remains_unresolved() {
        use crate::metrics::{cell::project, principal};
        use principal::retirement;

        let fixture = super::Fixture::new();
        let store = Store::open(&fixture.0, [1; 32]).unwrap();
        let target = store
            .root()
            .join("locks/projects")
            .join(format!("{}.pending", "ab".repeat(32)));
        fs::write(&target, b"").unwrap();
        let before = project::Manifest::read(store.root()).unwrap();
        let recorder = recorder::Recorder::transform(100).unwrap();
        let scope = recorder.scope().unwrap();
        let temporary = lease::with_publication(store.root(), store.graph_budget(), |authority| {
            let admitted = control::Admitted::seal(
                authority,
                store.root(),
                control::Plan::retirement(&target),
            )?;
            let mut observation = publication::Attempt::start(publication::Class::Retirement(
                publication::Retirement::Enrollment,
            ));
            let retirement = retirement::Attempt::start(
                authority,
                &target,
                retirement::Cause::Enrollment,
                retirement::Facts::default(),
            );
            entry::stage_admitted_retirement(
                admitted,
                authority,
                store.root(),
                &target,
                &mut observation,
                #[cfg(feature = "import-baseline")]
                Some(entry::Retirement::Allocation(&retirement)),
            )
        })
        .unwrap();
        drop(scope);
        let mut snapshot = recorder.snapshot().unwrap();
        snapshot
            .principal
            .as_mut()
            .unwrap()
            .prepare_cell_transport(&mut snapshot.accounting, 100)
            .unwrap();
        let decoded: principal::Evidence = serde_json::from_slice(
            &serde_json::to_vec(snapshot.principal.as_ref().unwrap()).unwrap(),
        )
        .unwrap();
        let after = project::Manifest::read(store.root()).unwrap();
        let other = principal::Evidence {
            cell_transport: true,
            ..principal::Evidence::default()
        };
        let pid = std::process::id();
        let joined = retirement::join::pair(
            [(pid, &decoded), (pid + 1, &other)],
            store.root(),
            &before,
            &after,
        )
        .unwrap();
        assert!(
            retirement::join::pair(
                [(pid + 2, &decoded), (pid + 1, &other)],
                store.root(),
                &before,
                &after
            )
            .is_err(),
            "the actual temporary and digest cannot be assigned to another reporting process"
        );
        assert_eq!(joined.attempts.len(), 1);
        assert_eq!(joined.attempts[0].finished, Some(false));
        assert!(joined.attempts[0].renamed.is_some());
        assert_eq!(joined.attempts[0].removal, None);
        assert_eq!(joined.missing.len(), 1);
        assert_eq!(joined.temporaries.len(), 1);
        assert_eq!(
            joined.temporaries[0].original.as_ref(),
            target
                .strip_prefix(store.root())
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
            "the renamed root retains its exact original path without a trailing separator"
        );
        assert!(joined.temporaries[0].matches_initial);
        let mut wire = serde_json::to_value(&decoded).unwrap();
        let row = wire["events"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["event"] == "retirement" && row["change"]["retirement"] == "relocated")
            .unwrap();
        row["change"]["temporary"]["sequence"] = serde_json::json!(u64::MAX);
        let altered: principal::Evidence = serde_json::from_value(wire).unwrap();
        assert!(retirement::join::pair(
            [(pid, &altered), (pid + 1, &other)],
            store.root(),
            &before,
            &after
        )
        .is_err());
        // Derive the failed-removal continuation from this real renamed
        // occurrence. It may retain descendants; success may not claim them gone.
        let token = joined.attempts[0].token;
        let mut rows = decoded.events.as_slice().to_vec();
        let terminal = rows.iter().position(|row| matches!(row, principal::Event::Retirement { change: retirement::Event::Finished { attempt, .. } } if *attempt == token)).unwrap();
        rows.insert(
            terminal,
            principal::Event::Retirement {
                change: retirement::Event::RemovalStarted { attempt: token },
            },
        );
        rows.insert(
            terminal + 1,
            principal::Event::Retirement {
                change: retirement::Event::RemovalFinished {
                    attempt: token,
                    succeeded: false,
                },
            },
        );
        let mut wire = serde_json::to_value(&decoded).unwrap();
        wire["events"] = serde_json::to_value(&rows).unwrap();
        let partial: principal::Evidence = serde_json::from_value(wire.clone()).unwrap();
        let partial = retirement::join::pair(
            [(pid, &partial), (pid + 1, &other)],
            store.root(),
            &before,
            &after,
        )
        .unwrap();
        assert_eq!(partial.attempts[0].removal, Some(false));
        assert_eq!(partial.temporaries.len(), 1);
        wire["events"][terminal + 1]["change"]["succeeded"] = serde_json::json!(true);
        let false_success: principal::Evidence = serde_json::from_value(wire).unwrap();
        assert!(retirement::join::pair(
            [(pid, &false_success), (pid + 1, &other)],
            store.root(),
            &before,
            &after
        )
        .is_err());
        let evidence = snapshot.accounting.publications.unwrap();
        let row = evidence
            .retirements
            .iter()
            .find(|row| row.purpose == publication::Retirement::Enrollment)
            .unwrap()
            .counts;
        assert_eq!(
            (row.created, row.unresolved, row.operation_failed),
            (1, 1, 1)
        );
        assert!(!evidence.closed());
        assert!(temporary.exists());
        assert!(!target.exists());
    }

    #[test]
    fn publication_history_recovery_adopts_existing_files_and_directories_without_creating_them() {
        use crate::metrics::{cell::project, principal};
        use std::os::unix::ffi::OsStrExt as _;
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = super::Fixture::new();
        let store = Store::open(&fixture.0, [1; 32]).unwrap();
        let temporary_root = store.root().join("temporaries");
        // Accepted names are decimal byte strings, not bounded process IDs.
        let old_name = format!("{}-{}", "0".repeat(48), "9".repeat(48));
        fs::write(temporary_root.join(&old_name), []).unwrap();
        let tree = temporary_root.join("1-2");
        fs::create_dir(&tree).unwrap();
        let payload = tree.join(std::ffi::OsStr::from_bytes(b"payload-\xc3\xa9"));
        fs::write(&payload, b"partial").unwrap();
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(&tree, fs::Permissions::from_mode(0o555)).unwrap();
        let before = project::Manifest::read(store.root()).unwrap();
        let recorder = recorder::Recorder::transform(1_000).unwrap();
        let scope = recorder.scope().unwrap();
        assert!(
            lease::with_publication(store.root(), store.graph_budget(), |publication| {
                recovery::recover_temporaries(
                    publication,
                    store.root(),
                    &mut recovery::Budget::new(),
                )
            })
            .unwrap()
        );
        drop(scope);
        let after = project::Manifest::read(store.root()).unwrap();
        let mut snapshot = recorder.snapshot().unwrap();
        let evidence = snapshot.accounting.publications.as_ref().unwrap();
        assert!(evidence.closed());
        for row in [evidence.recovered_files, evidence.recovered_directories] {
            assert_eq!((row.created, row.adopted, row.cleaned), (0, 1, 1));
        }
        let history = snapshot.principal.as_mut().unwrap();
        history
            .prepare_cell_transport(&mut snapshot.accounting, 1_000)
            .unwrap();
        let history: principal::Evidence =
            serde_json::from_slice(&serde_json::to_vec(history).unwrap()).unwrap();
        let empty = principal::Evidence {
            cell_transport: true,
            ..Default::default()
        };
        let processes = [(1, &history), (2, &empty)];
        let retirements =
            principal::retirement::join::pair(processes, store.root(), &before, &after).unwrap();
        let pins = principal::pin::join::pair(processes, store.root(), &before, &after).unwrap();
        let joined = principal::recovery::join::pair(
            processes,
            store.root(),
            &before,
            &after,
            (&retirements, &pins),
        )
        .unwrap();
        assert_eq!(joined.missing.len(), 3);
        assert_eq!(joined.modes.len(), 2);
        assert!(joined
            .modes
            .iter()
            .any(|mode| mode.path.ends_with(b"payload-\xc3\xa9")));
        assert!(joined
            .temporaries
            .iter()
            .all(|attempt| attempt.removed == Some(true)));
        assert_eq!(joined.batches[0].synced, Some(true));
        assert!(joined.batches[0].measured.iter().any(|row| !row.directory
            && row.bytes == 0
            && row.path.ends_with(old_name.as_bytes())));
        assert_eq!(fs::read_dir(&temporary_root).unwrap().count(), 0);

        let mut omitted = serde_json::to_value(&history).unwrap();
        let rows = omitted["events"].as_array_mut().unwrap();
        let first = rows
            .iter()
            .position(|row| row["event"] == "recovery" && row["change"]["recovery"] == "bytes")
            .unwrap();
        rows.remove(first);
        let omitted = serde_json::from_value::<principal::Evidence>(omitted).unwrap();
        assert!(principal::recovery::join::pair(
            [(1, &omitted), (2, &empty)],
            store.root(),
            &before,
            &after,
            (&retirements, &pins)
        )
        .is_err());

        // These failure transcripts derive from the real complete recovery above;
        // they are bounded join checks, not injected native failures.
        let mut failed_sync = serde_json::to_value(&history).unwrap();
        for row in failed_sync["events"].as_array_mut().unwrap() {
            if row["event"] != "recovery" {
                continue;
            }
            if row["change"]["recovery"] == "synced" {
                row["change"]["succeeded"] = false.into();
            }
            if row["change"]["recovery"] == "batch-finished" {
                row["change"]["recovered"] = serde_json::Value::Null;
            }
        }
        let failed_sync = serde_json::from_value::<principal::Evidence>(failed_sync).unwrap();
        let failed = principal::recovery::join::pair(
            [(1, &failed_sync), (2, &empty)],
            store.root(),
            &before,
            &after,
            (&retirements, &pins),
        )
        .unwrap();
        assert!(failed
            .temporaries
            .iter()
            .all(|attempt| attempt.removed == Some(true)));
        assert_eq!(failed.batches[0].synced, Some(false));

        let directory = joined
            .temporaries
            .iter()
            .find(|attempt| attempt.directory)
            .unwrap();
        let mut partial = serde_json::to_value(&history).unwrap();
        let rows = partial["events"].as_array_mut().unwrap();
        rows.retain(|row| !(row["event"] == "recovery" && row["change"]["recovery"] == "synced"));
        for row in rows {
            if row["event"] != "recovery" {
                continue;
            }
            if row["change"]["attempt"] == directory.token
                && (row["change"]["recovery"] == "removed"
                    || row["change"]["recovery"] == "finished")
            {
                row["change"]["succeeded"] = false.into();
            }
            if row["change"]["recovery"] == "batch-finished" {
                row["change"]["recovered"] = serde_json::Value::Null;
            }
        }
        let partial = serde_json::from_value::<principal::Evidence>(partial).unwrap();
        let mut retained = after.clone();
        let mut dirs = retained.directories.to_vec();
        let mut remaining = before
            .directories
            .iter()
            .find(|row| row.path == directory.path)
            .unwrap()
            .clone();
        remaining.mode = joined
            .modes
            .iter()
            .find(|mode| mode.path == directory.path)
            .unwrap()
            .requested
            & 0o7777;
        dirs.push(remaining);
        dirs.sort_by(|left, right| left.path.cmp(&right.path));
        retained.directories = dirs.into_boxed_slice();
        let partial = principal::recovery::join::pair(
            [(1, &partial), (2, &empty)],
            store.root(),
            &before,
            &retained,
            (&retirements, &pins),
        )
        .unwrap();
        assert_eq!(partial.retained.len(), 1);
        assert_eq!(partial.retained[0].matching_modes.len(), 1);
        assert!(
            principal::recovery::join::pair(
                processes,
                store.root(),
                &before,
                &retained,
                (&retirements, &pins)
            )
            .is_err(),
            "successful removal cannot explain a retained temporary directory"
        );

        // Append a later replacement to the real recovered-path history. Both
        // checked occurrence families may explain retention only while their
        // replacement is unresolved or partially removed. Successful removal
        // invalidates that explanation even if a final endpoint is injected.
        for pin in [false, true] {
            for removed in [None, Some(false), Some(true)] {
                let token = u64::MAX - 1;
                let key = [1; 32];
                let temporary = principal::retirement::Temporary {
                    process: 1,
                    sequence: 2,
                };
                assert_eq!(
                    temporary.relative().as_os_str().as_encoded_bytes(),
                    directory.path.as_ref()
                );
                let digest = principal::path_digest(&temporary.relative());
                let publication = joined.batches[0].publication;
                let mut rows = history.events.as_slice().to_vec();
                if pin {
                    let path = principal::ledger::Target::Project(key).relative();
                    rows.push(principal::Event::Pin {
                        change: principal::pin::Event::Started {
                            attempt: token,
                            publication,
                            source: None,
                            root: principal::path_digest(store.root()),
                            relative: principal::path_digest(&path),
                            operation: principal::pin::Operation::Project {
                                project: key,
                                purpose: publication::Replacement::ProjectRecovery,
                                cause: principal::ledger::Cause::Recovery(None),
                            },
                        },
                    });
                    rows.push(principal::Event::Pin {
                        change: principal::pin::Event::Relocated {
                            attempt: token,
                            temporary: digest,
                        },
                    });
                    if let Some(succeeded) = removed {
                        rows.push(principal::Event::Pin {
                            change: principal::pin::Event::Step {
                                attempt: token,
                                step: principal::pin::Step::RemovalStarted,
                            },
                        });
                        rows.push(principal::Event::Pin {
                            change: principal::pin::Event::Step {
                                attempt: token,
                                step: if succeeded {
                                    principal::pin::Step::Removed
                                } else {
                                    principal::pin::Step::RemovalFailed
                                },
                            },
                        });
                    }
                    rows.push(principal::Event::Pin {
                        change: principal::pin::Event::Finished {
                            attempt: token,
                            succeeded: removed == Some(true),
                        },
                    });
                } else {
                    let target = principal::retirement::Target::Enrollment { project: key };
                    rows.push(principal::Event::Retirement {
                        change: principal::retirement::Event::Started {
                            attempt: token,
                            publication,
                            root: principal::path_digest(store.root()),
                            relative: principal::path_digest(&target.relative().unwrap()),
                            target,
                            cause: principal::retirement::Cause::Enrollment,
                            facts: principal::retirement::Facts::default(),
                        },
                    });
                    rows.push(principal::Event::Retirement {
                        change: principal::retirement::Event::Relocated {
                            attempt: token,
                            temporary,
                            relative: digest,
                        },
                    });
                    if let Some(succeeded) = removed {
                        rows.push(principal::Event::Retirement {
                            change: principal::retirement::Event::RemovalStarted { attempt: token },
                        });
                        rows.push(principal::Event::Retirement {
                            change: principal::retirement::Event::RemovalFinished {
                                attempt: token,
                                succeeded,
                            },
                        });
                    }
                    rows.push(principal::Event::Retirement {
                        change: principal::retirement::Event::Finished {
                            attempt: token,
                            succeeded: removed == Some(true),
                        },
                    });
                }
                let mut wire = serde_json::to_value(&history).unwrap();
                wire["events"] = serde_json::to_value(rows).unwrap();
                let replacement = serde_json::from_value::<principal::Evidence>(wire).unwrap();
                let processes = [(1, &replacement), (2, &empty)];
                // The occurrence join is checked against the actual absent
                // endpoint first; then inject the retained endpoint solely at
                // the recovery join to exercise its independent guard.
                let retirements =
                    principal::retirement::join::pair(processes, store.root(), &before, &after)
                        .unwrap();
                let pins =
                    principal::pin::join::pair(processes, store.root(), &before, &after).unwrap();
                let result = principal::recovery::join::pair(
                    processes,
                    store.root(),
                    &before,
                    &retained,
                    (&retirements, &pins),
                );
                assert_eq!(result.is_ok(), removed != Some(true), "resolved replacement cannot excuse retained recovery endpoint: pin={pin}, removed={removed:?}");
            }
        }
    }
}

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-std-path-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }

    fn spelling(&self, length: usize) -> PathBuf {
        let mut parent = self.0.clone();
        while length - parent.as_os_str().as_encoded_bytes().len() - 1 > 200 {
            parent.push("d".repeat(150));
        }
        let remaining = length - parent.as_os_str().as_encoded_bytes().len() - 1;
        let path = parent.join("n".repeat(remaining));
        assert_eq!(path.as_os_str().as_encoded_bytes().len(), length);
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn budget(remaining: usize, paths: usize) -> GraphBudget {
    let budget = GraphBudget::new();
    budget
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - remaining)
        .unwrap();
    budget
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    budget
}

/// The decoded-bytes axis high water once the graph crossed its target.
fn decoded_high_water(budget: &GraphBudget) -> Option<u64> {
    budget
        .exceedances()
        .axis(node::DECODED_RESOURCE)
        .map(|axis| axis.high_water)
}

/// The path-check axis high water once the graph crossed its target.
fn paths_high_water(budget: &GraphBudget) -> Option<u64> {
    budget
        .exceedances()
        .axis(node::PATHS_RESOURCE)
        .map(|axis| axis.high_water)
}

/// `budget` sits exactly at its decoded target: nothing is recorded, and
/// one more byte is still charged while recording the exact cumulative total.
fn assert_exact_decoded(budget: &GraphBudget) {
    assert_eq!(decoded_high_water(budget), None);
    budget.decode(1).expect("an accounting axis never refuses");
    assert_eq!(
        decoded_high_water(budget),
        Some(node::MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
}

/// `budget` sits exactly at its path-check target.
fn assert_exact_paths(budget: &GraphBudget) {
    assert_eq!(paths_high_water(budget), None);
    budget.paths(1).expect("an accounting axis never refuses");
    assert_eq!(
        paths_high_water(budget),
        Some(node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
    );
}

/// One byte less remaining still completes the operation; the exceedance
/// records exactly one byte past the target, proving the exact charge.
fn assert_one_over_decoded(budget: &GraphBudget) {
    assert_eq!(
        decoded_high_water(budget),
        Some(node::MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
}

#[test]
fn directory_canonicalization_precharges_libc_and_copied_output() {
    let fixture = Fixture::new();
    for path in [
        fixture.0.clone(),
        fixture.spelling(383),
        fixture.spelling(384),
    ] {
        fs::create_dir_all(&path).unwrap();
        let length = path.as_os_str().as_encoded_bytes().len();
        let input = if length >= 384 { length + 1 } else { 0 };
        // Current PathBuf, optional CString, realpath's simultaneous libc and
        // copied Rust outputs, and the returned PathBuf's existing layout charge.
        let required = 2 * std::mem::size_of::<PathBuf>() + length + 1 + input + 1024 + 1023;
        let exact = budget(required, 1);
        assert_eq!(
            super::existing_directory_budgeted(&path, Path::new(""), &exact).unwrap(),
            Some(path.clone())
        );
        assert_exact_decoded(&exact);
        assert_exact_paths(&exact);

        let floor = budget(required - 1, 1);
        assert_eq!(
            super::existing_directory_budgeted(&path, Path::new(""), &floor).unwrap(),
            Some(path.clone())
        );
        assert_one_over_decoded(&floor);
    }
}

#[test]
fn directory_canonicalization_charges_before_observing_a_missing_path() {
    let fixture = Fixture::new();
    let path = fixture.0.join("absent");
    let required = 2 * std::mem::size_of::<PathBuf>()
        + path.as_os_str().as_encoded_bytes().len()
        + 1
        + 1024
        + 1023;
    // The complete canonicalization ownership is charged before the
    // missing path is observed. With one byte less remaining the observation
    // still runs and fails for its own reason, while the exceedance records the
    // exact charge; with the exact remaining nothing is recorded.
    let floor = budget(required - 1, 1);
    assert!(matches!(
        super::existing_directory_budgeted(&path, Path::new(""), &floor),
        Err(Error::Io {
            operation: "canonicalize typed store directory",
            ..
        })
    ));
    assert_one_over_decoded(&floor);
    let exact = budget(required, 1);
    assert!(matches!(
        super::existing_directory_budgeted(&path, Path::new(""), &exact),
        Err(Error::Io {
            operation: "canonicalize typed store directory",
            ..
        })
    ));
    assert_eq!(decoded_high_water(&exact), None);
}

#[test]
fn directory_walk_precharges_metadata_and_canonical_inputs_independently() {
    let fixture = Fixture::new();
    let root = fixture.spelling(384);
    let path = root.join("child");
    fs::create_dir_all(&path).unwrap();
    let length = path.as_os_str().as_encoded_bytes().len();
    let required = 2 * std::mem::size_of::<PathBuf>() + length + 2 * (length + 1) + 1024 + 1023;
    let exact = budget(required, 2);
    assert_eq!(
        super::existing_directory_budgeted(&root, Path::new("child"), &exact).unwrap(),
        Some(path)
    );
    assert_exact_decoded(&exact);
    assert_exact_paths(&exact);
    let floor = budget(required - 1, 2);
    assert_eq!(
        super::existing_directory_budgeted(&root, Path::new("child"), &floor).unwrap(),
        Some(root.join("child"))
    );
    assert_one_over_decoded(&floor);
}

#[test]
fn file_read_precharges_both_inputs_at_the_stack_threshold() {
    let fixture = Fixture::new();
    for length in [383, 384] {
        let path = fixture.spelling(length);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"four").unwrap();
        let input = if length >= 384 { length + 1 } else { 0 };
        let required = 4 + 2 * input;
        let exact = budget(required, 2);
        assert_eq!(
            &*super::read_file_budgeted(&path, 4, &exact).unwrap(),
            b"four"
        );
        assert_exact_decoded(&exact);
        assert_exact_paths(&exact);
        let floor = budget(required - 1, 2);
        assert_eq!(
            &*super::read_file_budgeted(&path, 4, &floor).unwrap(),
            b"four"
        );
        assert_one_over_decoded(&floor);
    }
}

#[test]
fn file_read_charges_long_input_before_observing_a_missing_path() {
    let fixture = Fixture::new();
    let path = fixture.spelling(384);
    // The 385-byte long-input ownership is charged before the missing
    // path is inspected. One byte short still inspects (and fails for its own
    // reason) while recording the exact charge; the exact amount records nothing.
    let floor = budget(384, 1);
    assert!(matches!(
        super::read_file_budgeted(&path, 4, &floor),
        Err(Error::Io {
            operation: "inspect typed store file",
            ..
        })
    ));
    assert_one_over_decoded(&floor);
    let exact = budget(385, 1);
    assert!(matches!(
        super::read_file_budgeted(&path, 4, &exact),
        Err(Error::Io {
            operation: "inspect typed store file",
            ..
        })
    ));
    assert_eq!(decoded_high_water(&exact), None);
}

#[test]
fn candidate_extent_rejection_keeps_direct_limits_and_prior_account_charges() {
    let fixture = Fixture::new();
    let path = fixture.0.join("payload");
    assert!(path.as_os_str().len() < 384);
    fs::write(&path, b"oversized").unwrap();
    assert!(matches!(
        super::read_file_budgeted(&path, 4, &GraphBudget::new()),
        Err(Error::Limit {
            resource: "typed store file bytes",
            limit: 4,
            actual: 9,
        })
    ));
    let graph = budget(0, 1);
    assert!(matches!(
        super::read_candidate_file_budgeted(&path, 4, &graph),
        Err(Error::Corrupt { .. })
    ));
    // The rejected extent spent exactly its one metadata probe and no
    // decoded ownership: both axes sit exactly at their targets.
    assert_exact_paths(&graph);
    assert_exact_decoded(&graph);

    // An exhausted path-check target no longer precedes the extent
    // observation. The probe is still charged first (recorded exactly one past
    // the target), the oversized extent is observed, and the callback's own
    // error is what the read reports.
    let graph = budget(0, 0);
    let mut observed = None;
    assert!(matches!(
        super::read_file_budgeted_with(&path, 4, &graph, |actual| {
            observed = Some((actual, paths_high_water(&graph)));
            Error::State("extent callback reports the oversized candidate")
        }),
        Err(Error::State(
            "extent callback reports the oversized candidate"
        ))
    ));
    assert_eq!(
        observed,
        Some((9, Some(node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)))
    );
    assert_eq!(decoded_high_water(&graph), None);

    // A genuine terminal failure raised by the callback is kept verbatim, and
    // the path check it charges on the shared account is recorded exactly.
    let graph = budget(0, 1);
    assert!(matches!(
        super::read_file_budgeted_with(&path, 4, &graph, |_| {
            graph.paths(1).expect("an accounting axis never refuses");
            Error::Allocation
        }),
        Err(Error::Allocation)
    ));
    assert_eq!(
        paths_high_water(&graph),
        Some(node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
    );
    assert_eq!(fs::read(path).unwrap(), b"oversized");
}
