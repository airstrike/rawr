use std::collections::VecDeque;
use std::ffi::CStr;

use super::*;
use crate::input::membership::{host, stored};
use crate::{cargo, input, node};

mod native;

const DIRECTORY: &[u8] = b"/p/\xff";
const LEGACY: &[u8] = b"/p/\xff/config";
const TOML: &[u8] = b"/p/\xff/config.toml";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Event {
    Open(Vec<u8>),
    Fstat(i32),
    Stat(Vec<u8>),
    Close(i32),
    Diagnostic(Diagnostic, Vec<i32>),
}

#[derive(Clone, Copy)]
enum Answer {
    Stat(Stat),
    Missing,
}

impl Answer {
    fn result(self) -> Result<Stat> {
        match self {
            Self::Stat(stat) => Ok(stat),
            Self::Missing => Err(Error::Unobserved),
        }
    }
}

struct Script {
    opens: VecDeque<std::result::Result<i32, i32>>,
    fstats: VecDeque<Answer>,
    fallback: Option<Answer>,
    events: Vec<Event>,
    live: Vec<i32>,
    max_live: usize,
}

fn metadata(inode: u64) -> Stat {
    Stat::Metadata(super::super::Metadata {
        kind: observed::Kind::File,
        device: 9,
        inode,
        owner: 0,
        mode: 0,
        length: 0,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}

impl Script {
    fn new(
        opens: impl IntoIterator<Item = std::result::Result<i32, i32>>,
        fstats: impl IntoIterator<Item = Stat>,
        fallback: Option<Stat>,
    ) -> Self {
        Self {
            opens: opens.into_iter().collect(),
            fstats: fstats.into_iter().map(Answer::Stat).collect(),
            fallback: fallback.map(Answer::Stat),
            events: Vec::new(),
            live: Vec::new(),
            max_live: 0,
        }
    }
}

impl System for Script {
    fn open(&mut self, path: &CStr) -> std::result::Result<i32, i32> {
        self.events.push(Event::Open(path.to_bytes().to_vec()));
        let result = self.opens.pop_front().expect("no eager open");
        if let Ok(descriptor) = result {
            assert!(!self.live.contains(&descriptor));
            self.live.push(descriptor);
            self.max_live = self.max_live.max(self.live.len());
            assert!(self.max_live <= 2);
        }
        result
    }

    fn fstat(&mut self, descriptor: i32) -> Result<Stat> {
        assert!(self.live.contains(&descriptor));
        self.events.push(Event::Fstat(descriptor));
        self.fstats.pop_front().expect("no eager fstat").result()
    }

    fn stat(&mut self, path: &CStr) -> Result<Stat> {
        assert!(
            self.live.is_empty(),
            "legacy failure closes before fallback"
        );
        self.events.push(Event::Stat(path.to_bytes().to_vec()));
        self.fallback.take().expect("no eager fallback").result()
    }

    fn close(&mut self, descriptor: i32) {
        let position = self
            .live
            .iter()
            .position(|fd| *fd == descriptor)
            .expect("close exactly once");
        self.live.remove(position);
        self.events.push(Event::Close(descriptor));
    }

    fn diagnostic(&mut self, intent: Diagnostic) {
        assert_ne!(intent, Diagnostic::None);
        assert!(
            self.live.contains(&17),
            "legacy survives logical warning boundary"
        );
        self.events
            .push(Event::Diagnostic(intent, self.live.clone()));
    }
}

fn run(script: &mut Script, warn: bool) -> Result<observed::Record> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    acquire(
        DIRECTORY,
        Filename::Config,
        warn,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        script,
    )
}

fn transcript(record: observed::Record) -> Transcript {
    let observed::Value::Discovery(value) = record.value else {
        panic!("discovery record")
    };
    value
}

fn open(path: &[u8]) -> Event {
    Event::Open(path.to_vec())
}
fn fallback() -> Event {
    Event::Stat(TOML.to_vec())
}

#[test]
fn exact_branches_preserve_overlap_warning_boundary_and_close_order() {
    let mut no_warn = Script::new([Ok(17)], [metadata(1)], None);
    assert_eq!(
        transcript(run(&mut no_warn, false).expect("legacy")).after(),
        After::None
    );
    assert_eq!(
        no_warn.events,
        [open(LEGACY), Event::Fstat(17), Event::Close(17)]
    );

    for (second, intent) in [
        (metadata(1), Diagnostic::None),
        (metadata(2), Diagnostic::Both),
        (Stat::Error(nix::libc::EINTR), Diagnostic::Deprecated),
    ] {
        let mut script = Script::new([Ok(17), Ok(23)], [metadata(1), second], None);
        let value = transcript(run(&mut script, true).expect("two acquisitions"));
        assert_eq!(value.diagnostic(), intent);
        let mut expected = vec![open(LEGACY), Event::Fstat(17), open(TOML), Event::Fstat(23)];
        if intent == Diagnostic::Deprecated {
            expected.push(Event::Close(23));
            expected.push(Event::Diagnostic(intent, vec![17]));
        } else {
            if intent == Diagnostic::Both {
                expected.push(Event::Diagnostic(intent, vec![17, 23]));
            }
            expected.push(Event::Close(23));
        }
        expected.push(Event::Close(17));
        assert_eq!(script.events, expected);
        assert_eq!(script.max_live, 2);
        assert!(script.live.is_empty());
    }

    let mut second_failure = Script::new([Ok(17), Err(107)], [metadata(1)], None);
    let value = transcript(run(&mut second_failure, true).expect("second native failure"));
    assert_eq!(
        value.after(),
        After::Second(Attempt::OpenFailed(NativeError::Os(107)))
    );
    assert_eq!(
        second_failure.events,
        [
            open(LEGACY),
            Event::Fstat(17),
            open(TOML),
            Event::Diagnostic(Diagnostic::Deprecated, vec![17]),
            Event::Close(17)
        ]
    );

    for first_stat in [false, true] {
        for warn in [false, true] {
            for exists in [metadata(1), Stat::Error(nix::libc::EINTR), Stat::Error(107)] {
                let mut script = if first_stat {
                    Script::new([Ok(17)], [Stat::Error(nix::libc::EINTR)], Some(exists))
                } else {
                    Script::new([Err(107)], [], Some(exists))
                };
                let value = transcript(run(&mut script, warn).expect("fallback"));
                let mut expected = vec![open(LEGACY)];
                if first_stat {
                    expected.extend([Event::Fstat(17), Event::Close(17)]);
                }
                expected.push(fallback());
                assert_eq!(script.events, expected);
                assert_eq!(value.diagnostic(), Diagnostic::None);
                assert_eq!(
                    value.after(),
                    After::Fallback(match exists {
                        Stat::Metadata(_) => Exists::Present,
                        Stat::Error(code) => Exists::Failed(NativeError::Os(code)),
                    })
                );
            }
        }
    }
}

#[test]
fn native_join_matches_fixed_source_basename_without_normalizing_directory() {
    use std::os::unix::ffi::OsStrExt as _;
    for directory in [
        b"/".as_slice(),
        b"//",
        b"/a",
        b"/a/",
        b"/a//",
        b"/a/./../\xff",
    ] {
        for filename in [Filename::Config, Filename::Credentials] {
            let limits = cargo::Limits::standard();
            let graph = node::GraphBudget::new();
            let mut allocation = input::Allocation::new(&limits);
            let mut script = Script::new([Err(2)], [], Some(Stat::Error(2)));
            let record = acquire(
                directory,
                filename,
                false,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            )
            .expect("native spelling");
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(directory));
            assert_eq!(
                script.events,
                [
                    open(path.join(filename.legacy()).as_os_str().as_bytes()),
                    Event::Stat(path.join(filename.toml()).as_os_str().as_bytes().to_vec())
                ]
            );
            let observed::Query::Discovery {
                directory: retained,
                ..
            } = record.query
            else {
                panic!("query")
            };
            assert_eq!(retained.bytes(), directory);
        }
    }
}

#[test]
fn no_native_type_gate_and_no_missing_or_invalid_error_fallback() {
    for kind in [
        observed::Kind::Directory,
        observed::Kind::Fifo,
        observed::Kind::Socket,
        observed::Kind::Character,
        observed::Kind::Unknown,
    ] {
        let Stat::Metadata(mut value) = metadata(1) else {
            panic!("metadata")
        };
        value.kind = kind;
        let mut script = Script::new([Err(13)], [], Some(Stat::Metadata(value)));
        assert_eq!(
            transcript(run(&mut script, false).expect("any metadata kind")).after(),
            After::Fallback(Exists::Present)
        );
        let mut script = Script::new([Ok(17)], [Stat::Metadata(value)], None);
        assert!(matches!(
            transcript(run(&mut script, false).expect("any openable kind")).legacy(),
            Attempt::Acquired(_)
        ));
    }
    for index in 0..3 {
        let mut script = match index {
            0 => Script::new([Ok(17)], [], None),
            1 => Script::new([Ok(17), Ok(23)], [metadata(1)], None),
            _ => Script::new([Err(13)], [], None),
        };
        if index == 2 {
            script.fallback = Some(Answer::Missing);
        } else {
            script.fstats.push_back(Answer::Missing);
        }
        assert!(matches!(run(&mut script, true), Err(Error::Unobserved)));
        assert!(!script
            .events
            .iter()
            .any(|event| matches!(event, Event::Diagnostic(..))));
        assert!(script.live.is_empty());
    }
    for code in [0, -1] {
        for mut script in [
            Script::new([Err(code)], [], None),
            Script::new([Ok(17)], [Stat::Error(code)], None),
            Script::new([Err(13)], [], Some(Stat::Error(code))),
            Script::new([Ok(17), Err(code)], [metadata(1)], None),
            Script::new([Ok(17), Ok(23)], [metadata(1), Stat::Error(code)], None),
        ] {
            assert!(matches!(run(&mut script, true), Err(Error::Encoding(_))));
            assert!(script.live.is_empty());
            assert!(!script
                .events
                .iter()
                .any(|event| matches!(event, Event::Diagnostic(..))));
        }
    }
}

fn scenario(index: usize) -> (Script, bool, usize) {
    match index {
        0 => (Script::new([Ok(17)], [metadata(1)], None), false, 3),
        1 => (
            Script::new([Ok(17), Ok(23)], [metadata(1), metadata(2)], None),
            true,
            6,
        ),
        2 => (Script::new([Err(13)], [], Some(metadata(1))), true, 3),
        3 => (
            Script::new([Ok(17)], [Stat::Error(4)], Some(metadata(1))),
            true,
            4,
        ),
        4 => (Script::new([Ok(17), Err(13)], [metadata(1)], None), true, 5),
        5 => (
            Script::new([Ok(17), Ok(23)], [metadata(1), Stat::Error(4)], None),
            true,
            6,
        ),
        6 => (
            Script::new(
                [Err(4), Err(4), Ok(17), Err(4), Ok(23)],
                [metadata(1), metadata(2)],
                None,
            ),
            true,
            9,
        ),
        _ => panic!("unknown fixture"),
    }
}

#[test]
fn every_graph_cut_preserves_first_refusal_and_prepaid_cleanup() {
    for index in 0..7 {
        let (mut baseline, warn, required) = scenario(index);
        run(&mut baseline, warn).expect("baseline");
        // The path-check target is measured, never a gate. Every cut
        // completes the acquisition with the baseline's events; a cut below
        // the required work records the exceedance with a truthful high water.
        for available in 0..=required {
            let (mut script, _, _) = scenario(index);
            let graph = node::GraphBudget::new();
            graph
                .paths(node::MAX_PATH_CHECKS_PER_GRAPH - available)
                .expect("prior work");
            let limits = cargo::Limits::standard();
            let mut allocation = input::Allocation::new(&limits);
            acquire(
                DIRECTORY,
                Filename::Config,
                warn,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            )
            .unwrap_or_else(|error| panic!("scenario {index}, work {available}: {error:?}"));
            assert!(script.live.is_empty());
            assert_eq!(script.events, baseline.events);
            let exceeded = graph.exceedances();
            if available == required {
                assert!(!exceeded.is_exceeded(), "scenario {index}");
            } else {
                let axis = exceeded.axis(node::PATHS_RESOURCE).unwrap();
                assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
                assert_eq!(
                    axis.first_requested,
                    node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
                );
                assert_eq!(
                    axis.high_water,
                    (node::MAX_PATH_CHECKS_PER_GRAPH + required - available) as u64,
                    "scenario {index}, work {available}"
                );
            }
            assert!(allocation.items(0).is_ok(), "nothing latched");
        }
        // Checked-arithmetic overflow of the graph account is a genuine
        // refusal: saturating the account leaves exactly `available` charges
        // before the next path check fails, before its syscall.
        for available in 0..=required {
            let (mut script, _, _) = scenario(index);
            let graph = node::GraphBudget::new();
            node::PATH_SITES.with(|sites| sites.borrow_mut().clear());
            graph
                .paths(usize::MAX - available)
                .expect("saturating prior work");
            let limits = cargo::Limits::standard();
            let mut allocation = input::Allocation::new(&limits);
            let result = acquire(
                DIRECTORY,
                Filename::Config,
                warn,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            );
            assert_eq!(
                result.is_ok(),
                available == required,
                "scenario {index}, work {available}"
            );
            if available < required {
                assert!(matches!(
                    result,
                    Err(Error::Budget(node::Error::Allocation))
                ));
            }
            assert!(script.live.is_empty());
            let physical = |events: &[Event]| {
                events
                    .iter()
                    .filter(|event| !matches!(event, Event::Close(_) | Event::Diagnostic(..)))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            let actual = physical(&script.events);
            assert_eq!(actual, physical(&baseline.events)[..actual.len()]);
            let opens = script
                .events
                .iter()
                .filter(|event| matches!(event, Event::Fstat(_)))
                .count();
            let closes = script
                .events
                .iter()
                .filter(|event| matches!(event, Event::Close(_)))
                .count();
            assert!(closes >= opens && closes <= 2);
            if available == required {
                assert_eq!(script.events, baseline.events);
                assert!(graph.paths(1).is_err());
            }
        }
    }
}

#[test]
fn all_allocation_cuts_preserve_reached_conversion_and_ownership_order() {
    for index in 0..7 {
        let (mut baseline, warn, _) = scenario(index);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut measured = input::Allocation::new(&limits);
        measured.items(7).expect("prior items");
        measured.bytes(19).expect("prior bytes");
        acquire(
            DIRECTORY,
            Filename::Config,
            warn,
            &mut Budget {
                graph: &graph,
                allocation: &mut measured,
            },
            &mut baseline,
        )
        .expect("baseline");
        let total = measured.usage();
        let conversions = usize::from(index != 0) + 1;
        assert_eq!(total.items, 7 + 3 + conversions);
        assert_eq!(
            total.bytes,
            19 + DIRECTORY.len()
                + LEGACY.len()
                + TOML.len()
                + LEGACY.len()
                + 1
                + if conversions == 2 { TOML.len() + 1 } else { 0 }
        );
        // Cumulative byte charges in ownership order: directory path, legacy
        // join, TOML join, terminated legacy path, then (when reached) the
        // terminated TOML path.
        let mut charges = vec![
            19 + DIRECTORY.len(),
            19 + DIRECTORY.len() + LEGACY.len(),
            19 + DIRECTORY.len() + LEGACY.len() + TOML.len(),
            19 + DIRECTORY.len() + LEGACY.len() + TOML.len() + LEGACY.len() + 1,
        ];
        if conversions == 2 {
            charges.push(charges[3] + TOML.len() + 1);
        }
        assert_eq!(*charges.last().unwrap(), total.bytes);
        // A byte target below the total is recorded, never refused. The
        // first crossing request identifies which ownership charge crossed it,
        // which fixes the charge amounts and their order.
        for bytes in 19..=total.bytes {
            let (mut script, _, _) = scenario(index);
            let graph = node::GraphBudget::new();
            let mut allocation =
                input::Allocation::with_limits(total.items, bytes, limits.string_bytes);
            allocation.items(7).expect("prior items");
            allocation.bytes(19).expect("prior bytes");
            acquire(
                DIRECTORY,
                Filename::Config,
                warn,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            )
            .expect("bytes past the target are owned");
            assert!(script.live.is_empty());
            assert_eq!(script.events, baseline.events);
            assert_eq!(allocation.usage(), total);
            let exceeded = allocation.exceedances();
            if bytes == total.bytes {
                assert!(!exceeded.is_exceeded());
            } else {
                let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                let crossing = charges
                    .iter()
                    .copied()
                    .find(|charge| *charge > bytes)
                    .unwrap();
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (bytes as u64, crossing as u64, total.bytes as u64)
                );
                assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
            }
        }
        // Checked-arithmetic overflow is a genuine refusal. Saturating the
        // account so that it overflows exactly where the old floor sat keeps
        // every cut: both early path allocations and the first native
        // conversion must finish before any native operation. Late cuts may
        // only keep the completed legacy operations and their mandatory cleanup.
        for bytes in 19..=total.bytes {
            let (mut script, _, _) = scenario(index);
            let graph = node::GraphBudget::new();
            let mut allocation =
                input::Allocation::with_limits(total.items, usize::MAX, limits.string_bytes);
            allocation.items(7).expect("prior items");
            allocation
                .bytes(usize::MAX - (bytes - 19))
                .expect("saturating prior bytes");
            let saturated = allocation.usage().bytes;
            let result = acquire(
                DIRECTORY,
                Filename::Config,
                warn,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            );
            assert_eq!(result.is_ok(), bytes == total.bytes);
            if bytes < total.bytes {
                assert!(matches!(
                    result,
                    Err(Error::Input(crate::Error::Node { ref source, .. }))
                        if matches!(**source, node::Error::Allocation)
                ));
            }
            assert!(script.live.is_empty());
            if bytes < charges[3] {
                assert!(script.events.is_empty());
            }
            assert!(allocation.usage().bytes - saturated <= bytes - 19);
        }
        for items in 7..=total.items {
            let (mut script, _, _) = scenario(index);
            let graph = node::GraphBudget::new();
            let mut allocation =
                input::Allocation::with_limits(items, total.bytes, limits.string_bytes);
            allocation.items(7).expect("prior items");
            allocation.bytes(19).expect("prior bytes");
            acquire(
                DIRECTORY,
                Filename::Config,
                warn,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            )
            .expect("items past the target are owned");
            assert!(script.live.is_empty());
            assert_eq!(script.events, baseline.events);
            assert_eq!(allocation.usage(), total);
            let exceeded = allocation.exceedances();
            if items == total.items {
                assert!(!exceeded.is_exceeded());
            } else {
                // Items are charged one at a time.
                let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (items as u64, items as u64 + 1, total.items as u64)
                );
            }
        }
        for items in 7..=total.items {
            let (mut script, _, _) = scenario(index);
            let graph = node::GraphBudget::new();
            let mut allocation =
                input::Allocation::with_limits(usize::MAX, total.bytes, limits.string_bytes);
            allocation
                .items(usize::MAX - (items - 7))
                .expect("saturating prior items");
            allocation.bytes(19).expect("prior bytes");
            let result = acquire(
                DIRECTORY,
                Filename::Config,
                warn,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                },
                &mut script,
            );
            assert_eq!(result.is_ok(), items == total.items);
            assert!(script.live.is_empty());
            if items < 11 {
                assert!(script.events.is_empty());
            }
        }
    }
}

#[test]
fn result_replays_under_the_same_original_account_without_current_authority() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut script = Script::new([Ok(17), Ok(23)], [metadata(1), metadata(2)], None);
    let record =
        acquire(DIRECTORY, Filename::Config, true, &mut budget, &mut script).expect("acquisition");
    let observed::Value::Discovery(expected) = &record.value else {
        panic!("discovery value")
    };
    let expected = *expected;
    let image = stored::Stored::from_records(&[record], &mut budget).expect("stored");
    let mut host = host::Host::from_image(&image, budget).expect("host");
    assert_eq!(
        host.discovery(DIRECTORY, Filename::Config, true)
            .expect("replayed"),
        expected
    );
    host.finish().expect("all records consumed");
}
