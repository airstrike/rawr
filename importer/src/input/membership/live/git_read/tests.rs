use std::{collections::VecDeque, ffi::CStr, os::unix::ffi::OsStrExt};

use super::{acquire, System};
use crate::input::membership::{
    git_read, host,
    live::{file, Metadata, Stat},
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget, Error, Result,
};
use crate::{cargo, input, node};

const PATH: &[u8] = b"/fixture/HEAD";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Native {
    Open,
    Length,
    Position,
    Read(usize),
    Directory,
    Close,
}

struct Script {
    contents: Vec<u8>,
    cursor: usize,
    reads: VecDeque<std::result::Result<usize, i32>>,
    opens: VecDeque<std::result::Result<i32, i32>>,
    lengths: VecDeque<std::result::Result<u64, i32>>,
    position: std::result::Result<u64, i32>,
    directory: std::result::Result<bool, i32>,
    events: Vec<Native>,
    live: bool,
    live_observer: std::rc::Rc<std::cell::Cell<bool>>,
}

fn metadata(length: u64, directory: bool) -> Metadata {
    Metadata {
        kind: if directory {
            observed::Kind::Directory
        } else {
            observed::Kind::File
        },
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0,
        length,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    }
}

impl Script {
    fn new(contents: &[u8]) -> Self {
        Self {
            contents: contents.to_vec(),
            cursor: 0,
            reads: VecDeque::new(),
            opens: VecDeque::new(),
            lengths: VecDeque::new(),
            position: Ok(0),
            directory: Ok(false),
            events: vec![],
            live: false,
            live_observer: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }
}

impl file::System for Script {
    fn open(&mut self, path: &CStr) -> std::result::Result<i32, i32> {
        assert_eq!(path.to_bytes(), PATH);
        assert!(!self.live);
        self.events.push(Native::Open);
        let value = self.opens.pop_front().unwrap_or(Ok(17));
        self.live = value.is_ok_and(|descriptor| descriptor >= 0);
        self.live_observer.set(self.live);
        value
    }
    fn fstat(&mut self, descriptor: i32) -> Result<Stat> {
        assert!(self.live);
        assert_eq!(descriptor, 17);
        self.events.push(Native::Length);
        Ok(
            match self
                .lengths
                .pop_front()
                .unwrap_or(Ok(self.contents.len() as u64))
            {
                Ok(length) => Stat::Metadata(metadata(length, false)),
                Err(code) => Stat::Error(code),
            },
        )
    }
    fn stat(&mut self, path: &CStr) -> Result<Stat> {
        assert!(self.live);
        assert_eq!(path.to_bytes(), PATH);
        self.events.push(Native::Directory);
        Ok(match self.directory {
            Ok(directory) => Stat::Metadata(metadata(0, directory)),
            Err(code) => Stat::Error(code),
        })
    }
    fn read(&mut self, descriptor: i32, bytes: &mut [u8]) -> std::result::Result<usize, i32> {
        assert!(self.live);
        assert_eq!(descriptor, 17);
        self.events.push(Native::Read(bytes.len()));
        let count = self.reads.pop_front().unwrap_or(Ok(bytes.len()))?;
        let count = count
            .min(bytes.len())
            .min(self.contents.len() - self.cursor);
        bytes[..count].copy_from_slice(&self.contents[self.cursor..self.cursor + count]);
        self.cursor += count;
        Ok(count)
    }
    fn close(&mut self, descriptor: i32) {
        assert!(self.live);
        assert_eq!(descriptor, 17);
        self.live = false;
        self.live_observer.set(false);
        self.events.push(Native::Close);
    }
}
impl System for Script {
    fn position(&mut self, descriptor: i32) -> std::result::Result<u64, i32> {
        assert!(self.live);
        assert_eq!(descriptor, 17);
        self.events.push(Native::Position);
        self.position
    }
}

fn capture(script: &mut Script, role: Role) -> observed::Record {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    acquire(
        PATH,
        role,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        script,
    )
    .unwrap()
}

pub(in crate::input::membership) fn head(contents: &[u8]) -> observed::Record {
    capture(&mut Script::new(contents), Role::LooseReference)
}

pub(in crate::input::membership) fn path(contents: &[u8]) -> observed::Record {
    capture(&mut Script::new(contents), Role::DiscoveryPath)
}

#[test]
fn configuration_reuses_caller_capacity_keeps_descriptor_during_completion_and_replays() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut buffer = git_read::Buffer::with_capacity(512, &mut budget).unwrap();
    let mut script = Script::new(&vec![b'x'; 700]);
    let live = script.live_observer.clone();
    let (record, ()) = super::configuration(
        PATH,
        &mut buffer,
        &mut budget,
        &mut script,
        |outcome, bytes, _| {
            assert!(live.get());
            assert!(matches!(outcome, git_read::Configuration::Read(Ok(()))));
            assert_eq!(bytes.bytes().len(), 700);
            Ok(())
        },
    )
    .unwrap();
    assert!(!live.get());
    assert_eq!(buffer.capacity(), 1024);
    assert_eq!(script.events.last(), Some(&Native::Close));
    let image = stored::Stored::from_records(&[record], &mut budget).unwrap();
    let wire = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&wire, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut replay = host.git_buffer().unwrap();
    host.git_configuration(
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(PATH)),
        &mut replay,
        |outcome, path, bytes, _| {
            assert!(path.is_some());
            assert!(matches!(outcome, git_read::Configuration::Read(Ok(()))));
            assert_eq!(bytes.bytes().len(), 700);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(replay.capacity(), buffer.capacity());
    host.finish().unwrap();
}

#[test]
fn configuration_open_failure_retains_buffer_and_completion_refusal_closes_once() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut buffer = git_read::Buffer::with_capacity(512, &mut budget).unwrap();
    let mut script = Script::new(b"retained");
    super::configuration(
        PATH,
        &mut buffer,
        &mut budget,
        &mut script,
        |_, _, _| Ok(()),
    )
    .unwrap();
    let mut denied = Script::new(b"");
    denied.opens = [Err(nix::libc::EACCES)].into();
    super::configuration(
        PATH,
        &mut buffer,
        &mut budget,
        &mut denied,
        |outcome, buffer, _| {
            assert!(matches!(
                outcome,
                git_read::Configuration::OpenFailed(nix::libc::EACCES)
            ));
            assert_eq!(buffer.bytes(), b"retained");
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(denied.events, [Native::Open]);
    let mut refused = Script::new(b"new");
    let live = refused.live_observer.clone();
    let result = super::configuration(
        PATH,
        &mut buffer,
        &mut budget,
        &mut refused,
        |_, _, budget| {
            assert!(live.get());
            budget.graph.paths(usize::MAX)?;
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!live.get());
    assert_eq!(
        refused
            .events
            .iter()
            .filter(|event| **event == Native::Close)
            .count(),
        1
    );
}

fn roundtrip(record: observed::Record, role: Role) -> git_read::Outcome {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[record], &mut budget).unwrap();
    let encoded = image.encode(&mut budget).unwrap();
    let decoded = stored::Stored::decode(&encoded, &mut budget).unwrap();
    assert_eq!(image, decoded);
    let mut host = host::Host::from_image(&decoded, budget).unwrap();
    let value = host.git_read(PATH, role).unwrap();
    host.finish().unwrap();
    value
}

#[test]
fn git_read_roundtrip_preserves_growth_hints_interruptions_and_exact_eof_probe() {
    for length in [0, 128, 129, 20000] {
        let bytes = vec![b'x'; length];
        let mut script = Script::new(&bytes);
        script.opens = [Err(nix::libc::EINTR), Ok(17)].into();
        script.lengths = [Err(nix::libc::EIO)].into();
        script.reads = [Err(nix::libc::EINTR)].into();
        let record = capture(&mut script, Role::LooseReference);
        assert_eq!(script.events.first(), Some(&Native::Open));
        assert_eq!(script.events.last(), Some(&Native::Close));
        assert!(!script.events.contains(&Native::Position));
        if length >= 128 {
            assert!(script.events.contains(&Native::Read(32)));
        }
        assert_eq!(
            roundtrip(record, Role::LooseReference),
            git_read::Outcome::Bytes(bytes)
        );
        assert!(!script.live);
    }
    let mut script = Script::new(b"abc");
    script.position = Err(nix::libc::EINTR);
    assert_eq!(
        roundtrip(
            capture(&mut script, Role::LooseReference),
            Role::LooseReference
        ),
        git_read::Outcome::Bytes(b"abc".to_vec())
    );
    assert_eq!(
        script
            .events
            .iter()
            .filter(|event| **event == Native::Position)
            .count(),
        1
    );
}

#[test]
fn git_read_errors_keep_open_distinct_and_probe_directory_before_close() {
    let mut denied = Script::new(b"");
    denied.opens = [Err(nix::libc::EACCES)].into();
    assert_eq!(
        roundtrip(
            capture(&mut denied, Role::LooseReference),
            Role::LooseReference
        ),
        git_read::Outcome::OpenFailed(nix::libc::EACCES)
    );
    assert_eq!(denied.events, [Native::Open]);
    for directory in [Ok(true), Ok(false), Err(nix::libc::ENOENT)] {
        let mut script = Script::new(b"prefix");
        script.directory = directory;
        script.reads = [Ok(3), Err(nix::libc::EIO)].into();
        let record = capture(&mut script, Role::LooseReference);
        let observed::Value::GitRead(transcript) = &record.value else {
            panic!("Git read record");
        };
        assert_eq!(transcript.bytes(), b"pre");
        assert_eq!(
            &script.events[script.events.len() - 2..],
            [Native::Directory, Native::Close]
        );
        assert_eq!(
            roundtrip(record, Role::LooseReference),
            git_read::Outcome::ReadFailed {
                code: nix::libc::EIO,
                directory: Some(directory)
            }
        );
    }
}

#[test]
fn git_read_path_gate_precedes_buffer_and_is_not_a_content_cap() {
    let mut oversized = Script::new(b"");
    oversized.lengths = [Ok(65537)].into();
    let outcome = roundtrip(
        capture(&mut oversized, Role::DiscoveryPath),
        Role::DiscoveryPath,
    );
    let git_read::Outcome::Oversized(error) = outcome else {
        panic!("owned size error")
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert_eq!(
        error.message(),
        Some(
            "Refusing to open files larger than 65536 bytes, '/fixture/HEAD' was 65537 bytes large"
        )
    );
    assert_eq!(
        oversized.events,
        [Native::Open, Native::Length, Native::Close]
    );
    let mut failed = Script::new(b"");
    failed.lengths = [Err(nix::libc::EINTR)].into();
    assert_eq!(
        roundtrip(
            capture(&mut failed, Role::DiscoveryPath),
            Role::DiscoveryPath
        ),
        git_read::Outcome::GateFailed(nix::libc::EINTR)
    );
    let contents = vec![b'a'; 65537];
    let mut changed = Script::new(&contents);
    changed.lengths = [Ok(0), Ok(0)].into();
    assert_eq!(
        roundtrip(
            capture(&mut changed, Role::DiscoveryPath),
            Role::DiscoveryPath
        ),
        git_read::Outcome::Bytes(contents)
    );
    assert_eq!(
        &changed.events[..4],
        [
            Native::Open,
            Native::Length,
            Native::Length,
            Native::Position
        ]
    );
}

#[test]
fn git_read_rejects_shape_and_reached_request_tampering() {
    assert!(Transcript::checked(
        Role::LooseReference,
        vec![
            Event::Open(Ok(())),
            Event::Length(Err(5)),
            Event::Read {
                requested: 128,
                result: Err(5)
            }
        ],
        vec![]
    )
    .is_err());
    let record = head(b"hi");
    let observed::Value::GitRead(transcript) = record.value else {
        panic!("Git read record");
    };
    let mut events = transcript.events().to_vec();
    for event in &mut events {
        if let Event::Read { requested, .. } = event {
            *requested += 1;
        }
    }
    let transcript =
        Transcript::checked(Role::LooseReference, events, transcript.bytes().to_vec()).unwrap();
    let record = observed::Record {
        query: record.query,
        value: observed::Value::GitRead(transcript),
    };
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[record], &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    assert!(matches!(
        host.git_read(PATH, Role::LooseReference),
        Err(Error::Encoding(_))
    ));
    assert!(host.finish().is_err());
}

#[test]
fn git_read_refusal_keeps_prepaid_cleanup_and_stops_native_work() {
    let limits = cargo::Limits::standard();
    let mut measured = input::Allocation::new(&limits);
    let graph = node::GraphBudget::new();
    let mut script = Script::new(b"prefix");
    script.reads = [Ok(3), Err(nix::libc::EIO)].into();
    acquire(
        PATH,
        Role::LooseReference,
        &mut Budget {
            graph: &graph,
            allocation: &mut measured,
        },
        &mut script,
    )
    .unwrap();
    let total = measured.usage();
    let baseline = script.events.clone();
    // Boundaries include initial buffer, event-vector growth and failed-byte
    // evidence. A byte target below the total is recorded, never
    // refused; the acquisition completes with the same events and one close.
    for bytes in [0, 64, 128, 256, total.bytes - 1, total.bytes] {
        let graph = node::GraphBudget::new();
        let mut allocation =
            input::Allocation::with_limits(total.items, bytes, limits.string_bytes);
        let mut script = Script::new(b"prefix");
        script.reads = [Ok(3), Err(nix::libc::EIO)].into();
        acquire(
            PATH,
            Role::LooseReference,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        )
        .expect("bytes past the target are owned");
        assert!(!script.live);
        assert_eq!(script.events, baseline);
        assert_eq!(allocation.usage(), total);
        let exceeded = allocation.exceedances();
        if bytes == total.bytes {
            assert!(!exceeded.is_exceeded());
        } else {
            let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
            assert_eq!(
                (axis.target, axis.high_water),
                (bytes as u64, total.bytes as u64)
            );
            assert!(axis.first_requested > bytes as u64);
        }
    }
    // Checked-arithmetic overflow is a genuine refusal at the same boundaries.
    // Every refused case must close any successfully acquired File.
    for bytes in [0, 64, 128, 256, total.bytes - 1, total.bytes] {
        let graph = node::GraphBudget::new();
        let mut allocation =
            input::Allocation::with_limits(total.items, usize::MAX, limits.string_bytes);
        allocation.bytes(usize::MAX - bytes).unwrap();
        let mut script = Script::new(b"prefix");
        script.reads = [Ok(3), Err(nix::libc::EIO)].into();
        let result = acquire(
            PATH,
            Role::LooseReference,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
            &mut script,
        );
        assert_eq!(result.is_ok(), bytes == total.bytes);
        assert!(!script.live);
        assert!(
            script
                .events
                .iter()
                .filter(|event| **event == Native::Close)
                .count()
                <= 1
        );
    }
    // Two path checks pay for cleanup and open; the descriptor-metadata check
    // crosses the target, which is recorded while the read completes.
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2).unwrap();
    let mut allocation = input::Allocation::new(&limits);
    let mut script = Script::new(b"abc");
    acquire(
        PATH,
        Role::LooseReference,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        &mut script,
    )
    .expect("path checks past the target are still made");
    assert!(!script.live);
    assert_eq!(script.events[..2], [Native::Open, Native::Length]);
    let axis = graph
        .exceedances()
        .axis(node::PATHS_RESOURCE)
        .copied()
        .unwrap();
    assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
    assert_eq!(
        axis.first_requested,
        node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
    );
    // Overflowing the graph account at the same point is a genuine refusal
    // that keeps the prepaid cleanup and stops native work.
    let graph = node::GraphBudget::new();
    graph.paths(usize::MAX - 2).unwrap();
    let mut allocation = input::Allocation::new(&limits);
    let mut script = Script::new(b"abc");
    assert!(matches!(
        acquire(
            PATH,
            Role::LooseReference,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation
            },
            &mut script
        ),
        Err(Error::Budget(node::Error::Allocation))
    ));
    assert_eq!(script.events, [Native::Open, Native::Close]);
}

#[test]
fn git_read_oversize_error_records_or_refuses_and_still_closes_the_live_descriptor_once() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut measured = input::Allocation::new(&limits);
    let mut script = Script::new(b"");
    script.lengths = [Ok(65537)].into();
    acquire(
        PATH,
        Role::DiscoveryPath,
        &mut Budget {
            graph: &graph,
            allocation: &mut measured,
        },
        &mut script,
    )
    .unwrap();
    let required = measured.usage();
    // One byte below the required ownership is a recorded exceedance;
    // the oversize error is still constructed and the descriptor closed once.
    let graph = node::GraphBudget::new();
    let mut allocation =
        input::Allocation::with_limits(required.items, required.bytes - 1, limits.string_bytes);
    let mut script = Script::new(b"");
    script.lengths = [Ok(65537)].into();
    acquire(
        PATH,
        Role::DiscoveryPath,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        &mut script,
    )
    .expect("bytes past the target are owned");
    assert_eq!(script.events, [Native::Open, Native::Length, Native::Close]);
    assert!(!script.live);
    assert_eq!(allocation.usage(), required);
    let axis = allocation
        .exceedances()
        .axis(input::INPUT_BYTES_RESOURCE)
        .copied()
        .unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (required.bytes as u64 - 1, required.bytes as u64)
    );
    // A genuine refusal while the oversize error is built: the account's
    // checked arithmetic overflowing on its last byte latches the account and
    // still closes the live descriptor once.
    let graph = node::GraphBudget::new();
    let mut allocation =
        input::Allocation::with_limits(required.items, usize::MAX, limits.string_bytes);
    allocation.bytes(usize::MAX - (required.bytes - 1)).unwrap();
    let mut script = Script::new(b"");
    script.lengths = [Ok(65537)].into();
    assert!(acquire(
        PATH,
        Role::DiscoveryPath,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation
        },
        &mut script
    )
    .is_err());
    assert_eq!(script.events, [Native::Open, Native::Length, Native::Close]);
    assert!(!script.live);
    assert!(allocation.check().is_err(), "overflow latches the account");
    // The oversize message crossing the string target is recorded on
    // the decoded-string axis; the error is still constructed with the same
    // ownership, the account never latches and the descriptor closes once.
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(required.items, usize::MAX, PATH.len());
    let mut script = Script::new(b"");
    script.lengths = [Ok(65537)].into();
    acquire(
        PATH,
        Role::DiscoveryPath,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
        &mut script,
    )
    .expect("the oversize message past the string target is owned");
    assert_eq!(script.events, [Native::Open, Native::Length, Native::Close]);
    assert!(!script.live);
    assert_eq!(allocation.usage(), required);
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    let message = format!(
        "Refusing to open files larger than {} bytes, '{}' was {} bytes large",
        65536,
        std::path::Path::new(std::ffi::OsStr::from_bytes(PATH)).display(),
        65537
    );
    let record = allocation.exceedances();
    let mut axes = record.axes();
    let axis = axes
        .next()
        .copied()
        .expect("the string target crossing is recorded");
    assert!(axes.next().is_none(), "{record:?}");
    assert_eq!(axis.name, "membership decoded TOML string bytes");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (
            PATH.len() as u64,
            message.len() as u64,
            message.len() as u64
        )
    );
}
