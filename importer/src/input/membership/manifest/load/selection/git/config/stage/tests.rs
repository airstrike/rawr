use super::*;
use crate::input::membership::{
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget,
};
use crate::{cargo, input, node};

fn transcript(bytes: &[u8], error: Option<i32>) -> Transcript {
    let mut events = vec![
        Event::Open(Ok(())),
        Event::Length(Ok(bytes.len() as u64)),
        Event::Position(Ok(0)),
    ];
    assert!(bytes.len() < 512);
    if !bytes.is_empty() {
        events.push(Event::Read {
            requested: 512,
            result: Ok(bytes.len()),
        });
    }
    events.push(Event::Read {
        requested: 512 - bytes.len(),
        result: error.map_or(Ok(0), Err),
    });
    Transcript::checked(
        Role::Configuration { capacity: 512 },
        events,
        bytes.to_vec(),
    )
    .unwrap()
}

fn run(
    local: Transcript,
    worktree: Option<Transcript>,
    refuse: bool,
) -> Result<Result<Stage, Failure>, Error<membership::Error>> {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records = vec![];
    for (name, transcript) in [("/r/config", Some(local)), ("/r/config.worktree", worktree)] {
        if let Some(transcript) = transcript {
            records.push(observed::Record {
                query: observed::Query::GitRead {
                    path: observed::Path::from_bytes(name.as_bytes(), &mut budget).unwrap(),
                    role: Role::Configuration { capacity: 512 },
                },
                value: observed::Value::GitRead(transcript),
            });
        }
    }
    let image = stored::Stored::from_records(&records, &mut budget).unwrap();
    let encoded = image.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut session = Session::new(&mut host);
    let result = if refuse {
        session.items(usize::MAX).and_then(|_| {
            Stage::local(
                Path::new("/r"),
                Path::new("/r"),
                trust::Level::Full,
                &mut session,
            )
        })
    } else {
        Stage::local(
            Path::new("/r"),
            Path::new("/r"),
            trust::Level::Full,
            &mut session,
        )
    };
    let result = session.finish(result);
    if matches!(result, Ok(Ok(_))) {
        host.finish().unwrap();
    }
    result
}

#[test]
fn codec_to_stage_keeps_pre_worktree_values_and_appends_complete_events() {
    let local=b"# local\n[core]\nbare=false\nrepositoryFormatVersion=1\n[extensions]\nobjectFormat=sha1\nworktreeConfig=true\n";
    let worktree=b"# worktree\n[CORE]\nbare=true\nprecomposeUnicode=tru\"e\"\nlogAllRefUpdates=always\n[gitoxide \"core\"]\nprotectWindows=yes\n";
    let stage = run(
        transcript(local, None),
        Some(transcript(worktree, None)),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(stage.is_bare, Some(false));
    assert!(stage.precompose_unicode);
    assert!(stage.protect_windows);
    assert_eq!(stage.reflog, Some(Reflog::Always));
    assert_eq!(stage.object_hash, Hash::Sha1);
    assert_eq!(stage.buffer.bytes(), worktree);
    assert_eq!(stage.config.order.iter().count(), 4);
    assert!(matches!(
        stage.config.front.first(),
        Some(super::super::Event::Comment { .. })
    ));
}

#[test]
fn malformed_input_and_format_version_errors_are_not_lenient_io_failures() {
    assert!(matches!(
        run(
            transcript(b"[core]\nvalue=\"unterminated", None),
            None,
            false
        )
        .unwrap(),
        Err(Failure::Parse(_))
    ));
    assert!(matches!(
        run(
            transcript(b"[extensions]\nobjectFormat=sha1\n", None),
            None,
            false
        )
        .unwrap(),
        Err(Failure::ObjectFormatRequiresV1)
    ));
    assert!(matches!(
        run(
            transcript(b"[core]\nrepositoryFormatVersion=-1\n", None),
            None,
            false
        )
        .unwrap(),
        Err(Failure::Unsigned(_))
    ));
    let partial = run(
        transcript(b"[core]\nbare=true\n", Some(nix::libc::EIO)),
        None,
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(partial.is_bare, None);
    assert!(partial.buffer.bytes().is_empty());
    assert!(matches!(
        run(transcript(b"", None), None, true),
        Err(Error::Admission(_))
    ));
}

#[test]
fn duplicate_case_keys_implicit_boolean_and_value_continuations_follow_source() {
    let local =
        b"[core]\nbare=false\nlogAllRefUpdates=always\n[CoRe]\nbare\nlogAllRefUpdates=tr\\\nue\n";
    let stage = run(transcript(local, None), None, false).unwrap().unwrap();
    assert_eq!(stage.is_bare, Some(true));
    assert_eq!(stage.reflog, Some(Reflog::Normal));
    assert_eq!(stage.config.order.iter().count(), 2);
}
