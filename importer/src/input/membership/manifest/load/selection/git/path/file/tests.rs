use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use super::{whitespace, Failure};
use crate::input::membership::{
    host, live,
    manifest::{io, session::Session},
    observed::{
        self,
        git_read::{Event, Role, Transcript},
    },
    stored, Budget,
};
use crate::{cargo, input, node};

enum Form {
    Gitdir,
    Plain,
    Relative,
}

fn consume(record: observed::Record, form: Form) -> Result<Option<PathBuf>, Failure> {
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
    let mut host = host::Host::from_image(&decoded, budget).unwrap();
    let mut session = Session::new(&mut host);
    let mut whitespace = whitespace::Owner::admit(&mut session).unwrap();
    let path = Path::new("/fixture/HEAD");
    let result = match form {
        Form::Gitdir => super::from_gitdir_file(path, &mut whitespace, &mut session)
            .map(|result| result.map(Some)),
        Form::Plain => super::from_plain_file(path, &mut whitespace, &mut session)
            .map(|result| result.transpose().map_err(Failure::Io)),
        Form::Relative => {
            super::from_plain_file_relative_to_file(path, &mut whitespace, &mut session)
                .map(|result| result.transpose().map_err(Failure::Io))
        }
    };
    let result = session.finish(result).unwrap();
    host.finish().unwrap();
    result
}

#[test]
fn path_consumers_connect_live_codec_replay_and_preserve_capacity_and_native_spelling() {
    let gitdir = consume(
        live::git_read::tests::path(b"gitdir: ../\xff\xe3\x80\x80\n"),
        Form::Gitdir,
    )
    .unwrap()
    .unwrap();
    assert_eq!(gitdir.as_os_str().as_bytes(), b"/fixture/../\xff");
    assert_eq!(gitdir.capacity(), 16);
    let absolute = consume(live::git_read::tests::path(b"gitdir: /x\r\n"), Form::Gitdir)
        .unwrap()
        .unwrap();
    assert_eq!(absolute, Path::new("/x"));
    assert_eq!(absolute.capacity(), 8); // copy parent before absolute replacement

    let plain = consume(live::git_read::tests::path(b" ../\xff\0 \t"), Form::Plain)
        .unwrap()
        .unwrap();
    assert_eq!(plain.as_os_str().as_bytes(), b" ../\xff\0");
    assert_eq!(plain.capacity(), 512); // same read Vec, only truncate + transfer
    let relative = consume(
        live::git_read::tests::path(b"../worktree\n"),
        Form::Relative,
    )
    .unwrap()
    .unwrap();
    assert_eq!(relative, Path::new("/fixture/../worktree"));
    assert_eq!(relative.capacity(), 32); // separator and suffix reserve separately
    let absolute = consume(live::git_read::tests::path(b"/worktree\n"), Form::Relative)
        .unwrap()
        .unwrap();
    assert_eq!(absolute, Path::new("/worktree"));
    assert_eq!(absolute.capacity(), 512);
}

fn native_error(events: Vec<Event>) -> observed::Record {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    observed::Record {
        query: observed::Query::GitRead {
            path: observed::Path::from_bytes(b"/fixture/HEAD", &mut budget).unwrap(),
            role: Role::DiscoveryPath,
        },
        value: observed::Value::GitRead(
            Transcript::checked(Role::DiscoveryPath, events, vec![]).unwrap(),
        ),
    }
}

#[test]
fn plain_files_treat_not_found_from_open_gate_and_read_as_absent_only() {
    for code in [libc::ENOENT, libc::EACCES] {
        for events in [
            vec![Event::Open(Err(code))],
            vec![Event::Open(Ok(())), Event::Length(Err(code))],
            vec![
                Event::Open(Ok(())),
                Event::Length(Ok(0)),
                Event::Length(Ok(0)),
                Event::Position(Ok(0)),
                Event::Read {
                    requested: 512,
                    result: Err(code),
                },
            ],
        ] {
            for form in [Form::Plain, Form::Relative] {
                let result = consume(native_error(events.clone()), form);
                if code == libc::ENOENT {
                    assert_eq!(result, Ok(None))
                } else {
                    assert_eq!(result, Err(Failure::Io(io::Failure::Native(code))))
                }
            }
            assert_eq!(
                consume(native_error(events), Form::Gitdir),
                Err(Failure::Io(io::Failure::Native(code)))
            );
        }
    }
}

#[test]
fn semantic_errors_own_whole_input_or_the_source_formatted_path_error() {
    for bytes in [
        b"gitdir:\t../bad".as_slice(),
        "gitdir: \u{3000}\n".as_bytes(),
    ] {
        assert_eq!(
            consume(live::git_read::tests::path(bytes), Form::Gitdir),
            Err(Failure::InvalidFormat {
                input: bytes.to_vec()
            })
        );
    }
    let Err(Failure::Io(error)) = consume(
        live::git_read::tests::path("\u{2000}\r\n".as_bytes()),
        Form::Plain,
    ) else {
        panic!("empty plain file error")
    };
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(
        error.message(),
        Some("Refusing to read an empty path from '/fixture/HEAD'")
    );

    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = Session::new(&mut budget);
    let result = super::relative(Path::new("/"), PathBuf::from("child"), &mut session);
    let error = session.finish(result).unwrap().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert_eq!(
        error.message(),
        Some("'/' has no parent, but 'child' is relative. It's impossible")
    );
}

#[test]
fn nul_path_is_a_local_io_error_with_source_conversion_ownership_and_no_host_query() {
    for length in [383usize, 384] {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(10_000, 10_000, 1024);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let image = stored::Stored::from_records(&[], &mut budget).unwrap();
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let before = host.budget().allocation.usage().bytes;
        let mut session = Session::new(&mut host);
        let mut whitespace = whitespace::Owner::admit(&mut session).unwrap();
        let mut bytes = vec![b'x'; length];
        bytes[0] = b'/';
        bytes[2] = 0;
        let path = Path::new(OsStr::from_bytes(&bytes));
        let result = super::from_plain_file(path, &mut whitespace, &mut session);
        assert_eq!(session.finish(result).unwrap(), Some(Err(io::Failure::Nul)));
        assert_eq!(
            host.budget().allocation.usage().bytes - before,
            if length < 384 { 0 } else { length + 1 }
        );
        host.finish().unwrap();
    }
}

#[test]
fn relative_parent_scan_refusal_precedes_join_and_absolute_paths_skip_it() {
    use crate::input::membership::manifest::Error;
    use rawr_toml::document::{Admission, Request};

    struct Scan {
        length: usize,
        calls: usize,
    }
    impl Admission for Scan {
        type Error = &'static str;
        fn admit(&mut self, request: Request) -> Result<(), Self::Error> {
            self.calls += 1;
            if request == Request::Items(self.length) {
                return Err("parent scan");
            }
            assert_eq!(
                request,
                Request::Items(1),
                "no join or error work before scan admission"
            );
            Ok(())
        }
    }
    let file = Path::new("/fixture////././gitdir");
    let mut account = Scan {
        length: file.as_os_str().len(),
        calls: 0,
    };
    let mut session = Session::new(&mut account);
    let result = super::relative(file, PathBuf::from("child"), &mut session);
    assert!(matches!(
        session.finish(result),
        Err(Error::Admission("parent scan"))
    ));
    assert_eq!(account.calls, 2);

    let mut account = Scan {
        length: file.as_os_str().len(),
        calls: 0,
    };
    let mut session = Session::new(&mut account);
    let result = super::relative(file, PathBuf::from("/worktree"), &mut session);
    assert_eq!(
        session.finish(result).unwrap().unwrap(),
        Path::new("/worktree")
    );
    assert_eq!(account.calls, 1);
}
