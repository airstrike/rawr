use std::collections::VecDeque;
use std::ffi::CStr;

use super::{file, Acquired, Comparison, Metadata, Stat};
use crate::input::membership::{document, host, observed, stored, Budget, Error, Result};
use crate::{cargo, input, node};

mod native;

const PATH: &[u8] = b"/fixture/Cargo.toml";

#[derive(Clone, Debug)]
enum Step {
    Bytes(Vec<u8>),
    Error(i32),
    Eof,
}

fn bytes(bytes: &[u8]) -> Step {
    Step::Bytes(bytes.to_vec())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Stat,
    Open,
    Fstat,
    Read(usize),
    Close,
}

struct Script {
    reads: VecDeque<Step>,
    opens: VecDeque<std::result::Result<i32, i32>>,
    path: Vec<u8>,
    path_stats: [Stat; 2],
    handle_stats: [Stat; 2],
    path_calls: usize,
    handle_calls: usize,
    events: Vec<Event>,
    /// Remaining `EINTR` replies for `open`, then `read` (a finite stream).
    interrupt_open: usize,
    interrupt_read: usize,
    live: bool,
}

fn metadata() -> Metadata {
    Metadata {
        kind: observed::Kind::File,
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0o100644,
        length: 999,
        modified_seconds: 4,
        modified_nanos: 5,
        changed_seconds: 6,
        changed_nanos: 7,
    }
}

impl Script {
    fn new(reads: impl IntoIterator<Item = Step>) -> Self {
        Self {
            reads: reads.into_iter().collect(),
            opens: VecDeque::new(),
            path: PATH.to_vec(),
            path_stats: [Stat::Metadata(metadata()); 2],
            handle_stats: [Stat::Metadata(metadata()); 2],
            path_calls: 0,
            handle_calls: 0,
            events: Vec::new(),
            interrupt_open: 0,
            interrupt_read: 0,
            live: false,
        }
    }

    fn closed(&self) -> usize {
        self.events
            .iter()
            .filter(|event| **event == Event::Close)
            .count()
    }
}

impl file::System for Script {
    fn stat(&mut self, path: &CStr) -> Result<Stat> {
        assert_eq!(path.to_bytes(), self.path);
        self.events.push(Event::Stat);
        let value = self.path_stats[self.path_calls];
        self.path_calls += 1;
        Ok(value)
    }

    fn open(&mut self, path: &CStr) -> std::result::Result<i32, i32> {
        assert_eq!(path.to_bytes(), self.path);
        assert!(!self.live);
        self.events.push(Event::Open);
        let result = if self.interrupt_open > 0 {
            self.interrupt_open -= 1;
            Err(libc::EINTR)
        } else {
            self.opens.pop_front().unwrap_or(Ok(17))
        };
        if result.is_ok() {
            self.live = true;
        }
        result
    }

    fn fstat(&mut self, descriptor: i32) -> Result<Stat> {
        assert_eq!(descriptor, 17);
        assert!(self.live);
        self.events.push(Event::Fstat);
        let value = self.handle_stats[self.handle_calls];
        self.handle_calls += 1;
        Ok(value)
    }

    fn read(&mut self, descriptor: i32, output: &mut [u8]) -> std::result::Result<usize, i32> {
        assert_eq!(descriptor, 17);
        assert!(self.live);
        assert!(!output.is_empty() && output.len() <= file::CHUNK);
        self.events.push(Event::Read(output.len()));
        if self.interrupt_read > 0 {
            self.interrupt_read -= 1;
            return Err(libc::EINTR);
        }
        match self
            .reads
            .pop_front()
            .expect("every read has a scripted outcome")
        {
            Step::Bytes(mut bytes) => {
                assert!(!bytes.is_empty());
                let count = bytes.len().min(output.len());
                output[..count].copy_from_slice(&bytes[..count]);
                if count < bytes.len() {
                    let rest = bytes.split_off(count);
                    self.reads.push_front(Step::Bytes(rest));
                }
                Ok(count)
            }
            Step::Error(code) => Err(code),
            Step::Eof => Ok(0),
        }
    }

    fn close(&mut self, descriptor: i32) {
        assert_eq!(descriptor, 17);
        assert!(self.live, "descriptor closed exactly once");
        self.live = false;
        self.events.push(Event::Close);
    }
}

fn run(script: &mut Script, limit: usize) -> Result<Acquired> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    file::acquire(
        PATH,
        limit,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        script,
    )
}

fn content(acquired: &Acquired) -> (&[u8], observed::End) {
    match &acquired.record().value {
        observed::Value::File { bytes, end } => (bytes, *end),
        other => panic!("file content expected: {other:?}"),
    }
}

#[test]
fn empty_and_exact_cap_require_real_nonempty_eof() {
    for (source, cap) in [(b"".as_slice(), 0), (b"abc".as_slice(), 3)] {
        let steps = if source.is_empty() {
            vec![Step::Eof]
        } else {
            vec![bytes(source), Step::Eof]
        };
        let mut script = Script::new(steps);
        let acquired = run(&mut script, cap).unwrap();
        assert_eq!(content(&acquired), (source, observed::End::Eof));
        assert_eq!(
            script
                .events
                .iter()
                .rev()
                .find(|event| matches!(event, Event::Read(_))),
            Some(&Event::Read(1))
        );
        assert_eq!(script.closed(), 1);
        assert!(script.reads.is_empty());
    }
}

#[test]
fn cap_plus_one_rejects_without_retaining_excess_or_post_read_io() {
    let mut script = Script::new([bytes(b"abcd"), Step::Eof]);
    let error = run(&mut script, 3).unwrap_err();
    assert!(matches!(
        error,
        Error::Input(crate::Error::Limit {
            limit: 3,
            actual: 4,
            ..
        })
    ));
    assert_eq!(
        script.events,
        [
            Event::Stat,
            Event::Open,
            Event::Fstat,
            Event::Read(3),
            Event::Read(1),
            Event::Close
        ]
    );
    assert_eq!(script.handle_calls, 1);
    assert_eq!(script.path_calls, 1);
    assert_eq!(script.reads.len(), 1);
}

#[test]
fn positive_short_reads_and_first_late_interruptions_continue_one_stream() {
    let mut script = Script::new([
        Step::Error(libc::EINTR),
        bytes(b"a"),
        bytes(b"b"),
        Step::Error(libc::EINTR),
        bytes(b"c"),
        Step::Eof,
    ]);
    script.opens = [Err(libc::EINTR), Ok(17)].into();
    let acquired = run(&mut script, 3).unwrap();
    assert_eq!(content(&acquired), (b"abc".as_slice(), observed::End::Eof));
    assert_eq!(
        script
            .events
            .iter()
            .filter(|event| **event == Event::Open)
            .count(),
        2
    );
    assert_eq!(
        script
            .events
            .iter()
            .filter(|event| matches!(event, Event::Read(_)))
            .count(),
        6
    );
    assert_eq!(script.closed(), 1);
}

#[test]
fn large_reads_use_fixed_chunks_and_a_separate_terminal_probe() {
    let source = vec![b'x'; file::CHUNK * 2 + 1];
    let mut script = Script::new([bytes(&source), Step::Eof]);
    let acquired = run(&mut script, source.len()).unwrap();
    assert_eq!(content(&acquired), (source.as_slice(), observed::End::Eof));
    let requests: Vec<_> = script
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Read(length) => Some(*length),
            _ => None,
        })
        .collect();
    assert_eq!(requests, [file::CHUNK, file::CHUNK, 1, 1]);
    assert_eq!(script.closed(), 1);
}

#[test]
fn terminal_errors_preserve_errno_and_never_promote_read_enoent_to_absence() {
    for code in [libc::EIO, libc::ENOENT, 107] {
        for prefix in [b"".as_slice(), b"a=1\n".as_slice(), b"\xff".as_slice()] {
            let mut steps = Vec::new();
            if !prefix.is_empty() {
                steps.push(bytes(prefix));
            }
            steps.push(Step::Error(code));
            let mut script = Script::new(steps);
            let acquired = run(&mut script, 100).unwrap();
            if prefix.is_empty() {
                assert_eq!(acquired.record().value, observed::Value::Error(code));
            } else {
                assert_eq!(content(&acquired), (prefix, observed::End::Error(code)));
            }
            assert_eq!(script.closed(), 1);

            let limits = cargo::Limits::standard();
            let graph = node::GraphBudget::new();
            let mut allocation = input::Allocation::new(&limits);
            let mut budget = Budget {
                graph: &graph,
                allocation: &mut allocation,
            };
            let image =
                stored::Stored::from_records(std::slice::from_ref(acquired.record()), &mut budget)
                    .unwrap();
            let mut host = host::Host::from_image(&image, budget).unwrap();
            assert!(matches!(host.file(PATH), Err(Error::Io(actual)) if actual == code));
            host.finish().unwrap();
        }
    }
}

#[test]
fn only_open_enoent_is_absence_and_no_descriptor_facts_are_fabricated() {
    for code in [
        libc::ENOENT,
        libc::ENOTDIR,
        libc::EACCES,
        107,
    ] {
        let mut script = Script::new([]);
        script.opens.push_back(Err(code));
        let acquired = run(&mut script, 100).unwrap();
        let expected = if code == libc::ENOENT {
            observed::Value::Absent
        } else {
            observed::Value::Error(code)
        };
        assert_eq!(acquired.record().value, expected);
        assert_eq!(acquired.identity().opened(), None);
        assert_eq!(acquired.identity().after_opened(), None);
        assert_eq!(acquired.identity().comparison(), Comparison::Unavailable);
        assert_eq!(script.events, [Event::Stat, Event::Open, Event::Stat]);
        assert_eq!(script.closed(), 0);
    }
}

#[test]
fn metadata_hints_do_not_bound_content_or_add_a_source_type_requirement() {
    for length in [0, 1, 3, u64::MAX] {
        for kind in [
            observed::Kind::File,
            observed::Kind::Character,
            observed::Kind::Fifo,
            observed::Kind::Unknown,
        ] {
            let meta = Metadata {
                length,
                kind,
                ..metadata()
            };
            let mut script = Script::new([bytes(b"abc"), Step::Eof]);
            script.path_stats = [Stat::Metadata(meta); 2];
            script.handle_stats = [Stat::Metadata(meta); 2];
            let acquired = run(&mut script, 3).unwrap();
            assert_eq!(content(&acquired), (b"abc".as_slice(), observed::End::Eof));
            assert_eq!(acquired.identity().comparison(), Comparison::Matched);
        }
    }
}

#[test]
fn growth_truncation_replacement_and_available_disagreement_are_explicit() {
    for changed in [
        Metadata {
            length: 0,
            ..metadata()
        },
        Metadata {
            length: 1000,
            ..metadata()
        },
        Metadata {
            inode: 99,
            ..metadata()
        },
        Metadata {
            device: 99,
            ..metadata()
        },
        Metadata {
            owner: 99,
            ..metadata()
        },
        Metadata {
            mode: 0o100600,
            ..metadata()
        },
        Metadata {
            modified_nanos: 99,
            ..metadata()
        },
        Metadata {
            changed_nanos: 99,
            ..metadata()
        },
    ] {
        let mut script = Script::new([bytes(b"abc"), Step::Eof]);
        script.path_stats[1] = Stat::Metadata(changed);
        script.handle_stats[1] = Stat::Error(libc::EIO);
        let acquired = run(&mut script, 3).unwrap();
        assert_eq!(content(&acquired).1, observed::End::Eof);
        assert_eq!(acquired.identity().comparison(), Comparison::Changed);
        assert_eq!(acquired.identity().after_path(), &Stat::Metadata(changed));
    }
}

#[test]
fn optional_metadata_errors_stay_optional_and_unknown_never_matches() {
    for position in 0..4 {
        let mut script = Script::new([bytes(b"a"), Step::Eof]);
        match position {
            0 => script.path_stats[0] = Stat::Error(107),
            1 => script.handle_stats[0] = Stat::Error(107),
            2 => script.handle_stats[1] = Stat::Error(107),
            3 => script.path_stats[1] = Stat::Error(107),
            _ => unreachable!(),
        }
        let acquired = run(&mut script, 1).unwrap();
        assert_eq!(content(&acquired), (b"a".as_slice(), observed::End::Eof));
        assert_eq!(acquired.identity().comparison(), Comparison::Unavailable);
    }
    let mut script = Script::new([Step::Error(libc::EACCES)]);
    script.path_stats = [Stat::Error(107); 2];
    script.handle_stats = [Stat::Error(107); 2];
    let acquired = run(&mut script, 1).unwrap();
    assert_eq!(
        acquired.record().value,
        observed::Value::Error(libc::EACCES)
    );
    assert_eq!(acquired.identity().before_path(), &Stat::Error(107));
    assert_eq!(acquired.identity().comparison(), Comparison::Unavailable);
}

#[test]
fn exact_native_query_keeps_non_utf8_bytes_and_overlong_spelling() {
    for path in [
        b"/a/../b/\xff".to_vec(),
        [b"/".as_slice(), &vec![b'x'; 4096]].concat(),
    ] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut script = Script::new([]);
        script.path = path.clone();
        script.opens.push_back(Err(libc::ENAMETOOLONG));
        let acquired = file::acquire(
            &path,
            1,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        )
        .unwrap();
        let observed::Query::File(retained) = &acquired.record().query else {
            panic!("file query")
        };
        assert_eq!(retained.bytes(), path);
        assert_eq!(
            acquired.record().value,
            observed::Value::Error(libc::ENAMETOOLONG)
        );
    }
}

#[test]
fn original_prior_ownership_exact_and_floor_limits_cover_full_replacements() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut measured = input::Allocation::new(&limits);
    measured.items(7).unwrap();
    measured.bytes(19).unwrap();
    let steps = [
        bytes(b"a"),
        bytes(b"b"),
        bytes(b"cd"),
        bytes(b"e"),
        Step::Eof,
    ];
    let mut script = Script::new(steps.clone());
    file::acquire(
        PATH,
        8,
        &mut Budget {
            graph: &graph,
            allocation: &mut measured,
        },
        &mut script,
    )
    .unwrap();
    let total = measured.usage();
    assert_eq!(total.items, 7 + 3); // Query path, terminated path, content vector.
    assert_eq!(
        total.bytes,
        19 + PATH.len() + PATH.len() + 1 + 1 + 2 + 4 + 8
    );
    // One less on either axis still acquires the complete file and
    // records that axis; the final eight-byte replacement layout is the
    // crossing request for the byte floor, proving full-layout charging.
    for (items, bytes, axis_name) in [
        (total.items, total.bytes, None),
        (
            total.items - 1,
            total.bytes,
            Some(input::INPUT_ITEMS_RESOURCE),
        ),
        (
            total.items,
            total.bytes - 1,
            Some(input::INPUT_BYTES_RESOURCE),
        ),
    ] {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, limits.string_bytes);
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let mut script = Script::new(steps.clone());
        file::acquire(
            PATH,
            8,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        )
        .expect("ownership targets never refuse");
        assert_eq!(script.closed(), 1);
        assert_eq!(allocation.usage(), total);
        let exceeded = allocation.exceedances();
        match axis_name {
            None => assert!(!exceeded.is_exceeded()),
            Some(name) => {
                let axis = exceeded.axis(name).unwrap();
                let exact = if name == input::INPUT_ITEMS_RESOURCE {
                    total.items
                } else {
                    total.bytes
                };
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (exact as u64 - 1, exact as u64, exact as u64)
                );
                assert_eq!(exceeded.axes().count(), 1);
            }
        }
        assert!(allocation.items(0).is_ok(), "nothing latched");
    }
}

#[test]
fn every_graph_floor_refuses_before_the_next_syscall_and_still_closes() {
    let mut baseline = Script::new([bytes(b"abc"), Step::Eof]);
    run(&mut baseline, 3).unwrap();
    let required = baseline.events.len();
    // The path-check target is measured, never a gate. Every cut below
    // the required work completes the acquisition with the baseline's events
    // and records the exceedance with a truthful high water.
    for available in 0..=required {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH - available)
            .unwrap();
        let mut allocation = input::Allocation::new(&limits);
        let mut script = Script::new([bytes(b"abc"), Step::Eof]);
        file::acquire(
            PATH,
            3,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        )
        .expect("path checks past the target are still made");
        assert_eq!(script.events, baseline.events);
        assert!(!script.live);
        assert_eq!(script.closed(), 1);
        let exceeded = graph.exceedances();
        if available == required {
            assert!(!exceeded.is_exceeded());
        } else {
            let axis = exceeded.axis(node::PATHS_RESOURCE).unwrap();
            assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
            assert_eq!(
                axis.first_requested,
                node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            );
            assert_eq!(
                axis.high_water,
                (node::MAX_PATH_CHECKS_PER_GRAPH + required - available) as u64
            );
        }
        assert!(allocation.items(0).is_ok(), "nothing latched");
    }
    // Checked-arithmetic overflow of the graph account is a genuine refusal:
    // saturating it leaves exactly `available` charges before the next path
    // check fails ahead of its syscall, and the descriptor still closes.
    for available in 0..=required {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        node::PATH_SITES.with(|sites| sites.borrow_mut().clear());
        graph.paths(usize::MAX - available).unwrap();
        let mut allocation = input::Allocation::new(&limits);
        let mut script = Script::new([bytes(b"abc"), Step::Eof]);
        let result = file::acquire(
            PATH,
            3,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        );
        assert_eq!(result.is_ok(), available == required);
        let ordinary: Vec<_> = script
            .events
            .iter()
            .copied()
            .filter(|event| *event != Event::Close)
            .collect();
        let expected: Vec<_> = baseline
            .events
            .iter()
            .copied()
            .filter(|event| *event != Event::Close)
            .take(ordinary.len())
            .collect();
        assert_eq!(ordinary, expected);
        assert!(!script.live);
        assert_eq!(
            script.closed(),
            usize::from(script.events.contains(&Event::Open))
        );
        if result.is_err() {
            assert!(matches!(
                result,
                Err(Error::Budget(node::Error::Allocation))
            ));
        }
    }
}

/// Every interrupted open or read attempt is charged to the original
/// attempt's path account, and charging past the target records an exceedance
/// instead of refusing. A finite interruption stream therefore completes, with
/// the exceedance's high water proving one path check per retried attempt.
#[test]
fn interrupted_open_and_read_charge_the_original_attempt_budget_past_its_target() {
    const INTERRUPTIONS: usize = 7;
    for opening in [true, false] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 9).unwrap();
        let mut allocation = input::Allocation::new(&limits);
        let mut script = Script::new([bytes(b"xyz"), Step::Eof]);
        script.interrupt_open = if opening { INTERRUPTIONS } else { 0 };
        script.interrupt_read = if opening { 0 } else { INTERRUPTIONS };
        let result = file::acquire(
            PATH,
            3,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(script.closed(), 1);
        // One path stat before the open and one after the read.
        assert_eq!(
            script
                .events
                .iter()
                .filter(|event| **event == Event::Stat)
                .count(),
            2
        );
        assert_eq!(
            script
                .events
                .iter()
                .filter(|event| **event == Event::Open)
                .count(),
            if opening { INTERRUPTIONS + 1 } else { 1 }
        );
        assert!(!script.live);
        let (_, paths, _) = graph.census_usage().unwrap();
        assert!(paths > node::MAX_PATH_CHECKS_PER_GRAPH);
        let exceeded = graph.exceedances();
        let axis = exceeded
            .axis(node::PATHS_RESOURCE)
            .expect("the retried attempts cross the path-check target");
        assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
        assert_eq!(
            axis.first_requested,
            node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert_eq!(axis.high_water, paths as u64, "truthful high water");
        assert!(
            allocation.items(0).is_ok(),
            "an exceedance never latches the account"
        );
    }
}

#[test]
fn allocation_refusal_precedes_reservation_and_cannot_spend_allocator_bonus() {
    // A genuine refusal is the account's checked arithmetic overflowing
    // on the two-byte replacement layout; it precedes the reservation even
    // though the vector already holds allocator bonus capacity.
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(100, usize::MAX, 100);
    allocation.bytes(usize::MAX - 1).unwrap();
    file::RESERVATIONS.with(|count| count.set(0));
    assert!(matches!(
        file::bonus_refusal(&mut Budget {
            graph: &graph,
            allocation: &mut allocation
        }),
        Err(Error::Input(crate::Error::Node { ref source, .. }))
            if matches!(**source, node::Error::Allocation)
    ));
    assert_eq!(file::RESERVATIONS.with(std::cell::Cell::get), 0);
    assert!(matches!(
        allocation.items(0),
        Err(crate::Error::InputAccountRefused)
    ));
    // Crossing the one-byte target instead is recorded; the full layout is
    // still charged (bonus capacity buys no bypass) and the reservation made.
    let mut allocation = input::Allocation::with_limits(100, 1, 100);
    file::RESERVATIONS.with(|count| count.set(0));
    file::bonus_refusal(&mut Budget {
        graph: &graph,
        allocation: &mut allocation,
    })
    .expect("bytes past the target are owned");
    assert_eq!(file::RESERVATIONS.with(std::cell::Cell::get), 1);
    let axis = allocation
        .exceedances()
        .axis(input::INPUT_BYTES_RESOURCE)
        .copied()
        .unwrap();
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (1, 2, 2)
    );

    // The terminated native path overflows the account before any syscall or
    // reservation.
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::with_limits(100, usize::MAX, limits.string_bytes);
    allocation
        .bytes(usize::MAX - (PATH.len() + PATH.len()))
        .unwrap();
    file::RESERVATIONS.with(|count| count.set(0));
    let mut script = Script::new([]);
    assert!(matches!(
        file::acquire(
            PATH,
            1,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation
            },
            &mut script
        ),
        Err(Error::Input(crate::Error::Node { ref source, .. }))
            if matches!(**source, node::Error::Allocation)
    ));
    assert!(script.events.is_empty());
    assert_eq!(file::RESERVATIONS.with(std::cell::Cell::get), 0);
    // Crossing the same byte target is recorded and the acquisition proceeds
    // to its syscalls.
    let mut allocation =
        input::Allocation::with_limits(100, PATH.len() + PATH.len(), limits.string_bytes);
    let mut script = Script::new([]);
    script.opens.push_back(Err(libc::ENOENT));
    let acquired = file::acquire(
        PATH,
        1,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        &mut script,
    )
    .expect("bytes past the target are owned");
    assert_eq!(acquired.record().value, observed::Value::Absent);
    assert_eq!(script.events, [Event::Stat, Event::Open, Event::Stat]);
    assert_eq!(file::RESERVATIONS.with(std::cell::Cell::get), 1);
    let axis = allocation
        .exceedances()
        .axis(input::INPUT_BYTES_RESOURCE)
        .copied()
        .unwrap();
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (
            (PATH.len() + PATH.len()) as u64,
            (PATH.len() + PATH.len() + 1) as u64,
            (PATH.len() + PATH.len() + 1) as u64
        )
    );
}

#[test]
fn pure_copy_identity_and_growth_do_not_spend_hash_decode_or_candidate_accounts() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    for _ in 0..64 {
        graph.comparison().unwrap();
    }
    let mut allocation = input::Allocation::new(&limits);
    let mut script = Script::new([bytes(b"a"), bytes(b"b"), Step::Eof]);
    let acquired = file::acquire(
        PATH,
        2,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        &mut script,
    )
    .unwrap();
    assert_eq!(acquired.identity().comparison(), Comparison::Matched);
    assert_eq!(content(&acquired).0, b"ab");
}

#[test]
fn acquired_record_flows_through_stored_host_and_document_under_one_account() {
    let source = b"[package]\nname='demo'\nversion='1.2.3'\n[unused]\nx=[1,{y=true}]\n";
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(7).unwrap();
    allocation.bytes(19).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut script = Script::new([bytes(source), Step::Eof]);
    let acquired = file::acquire(PATH, limits.manifest_bytes, &mut budget, &mut script).unwrap();
    let image =
        stored::Stored::from_records(std::slice::from_ref(acquired.record()), &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let file = host.file(PATH).unwrap().unwrap();
    let document = document::parse(file, &limits, host.budget()).unwrap();
    assert_eq!(document.source().as_bytes(), source);
    assert!(document.table().get("unused").is_some());
    host.finish().unwrap();
    assert_eq!(script.closed(), 1);
}
