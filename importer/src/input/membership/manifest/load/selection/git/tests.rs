use std::{
    borrow::Cow,
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use rawr_toml::document;

use crate::input::membership::manifest::{reference::Account, session::Session};

use super::{path, repository};

#[path = "reference.rs"]
mod pinned;

fn native(bytes: &[u8]) -> &Path {
    Path::new(OsStr::from_bytes(bytes))
}

fn bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

#[test]
fn normalization_matches_pinned_bytes_and_conditional_ownership() {
    let inputs: &[&[u8]] = &[
        b"",
        b".",
        b"..",
        b"../..",
        b"../../..",
        b"/",
        b"//",
        b"/..",
        b"/a/../../b",
        b"./a//b/",
        b"./a/..",
        b"a/../..",
        b"a/../../b",
        b"//a///b//../c/",
        b"../base",
        b"../base/..",
        b".git",
        b"a.git",
        b"a/../.git",
        b"./a\xff/../b",
        b"a\0b/../\xff",
        b"\\a\\b",
        "λ/../β".as_bytes(),
    ];
    let mut account = Account;
    let mut session = Session::new(&mut account);
    for &cwd in &[
        b"/".as_slice(),
        b"/base",
        b"/a/base/",
        b"/a//base",
        b"/\xff",
    ] {
        for &input in inputs {
            for owned in [false, true] {
                let input = native(input);
                let cwd = native(cwd);
                let make = || {
                    if owned {
                        Cow::Owned(input.to_owned())
                    } else {
                        Cow::Borrowed(input)
                    }
                };
                let actual = path::normalize(make(), cwd, &mut session).unwrap();
                let expected = pinned::normalize(make(), cwd);
                assert_eq!(
                    actual.as_deref().map(bytes),
                    expected.as_deref().map(bytes),
                    "{input:?}, cwd {cwd:?}"
                );
                assert_eq!(
                    matches!(actual, Some(Cow::Owned(_))),
                    matches!(expected, Some(Cow::Owned(_)))
                );
                if let (Some(Cow::Owned(actual)), Some(Cow::Owned(expected))) = (&actual, &expected)
                {
                    assert_eq!(
                        actual.capacity(),
                        expected.capacity(),
                        "{input:?}, cwd {cwd:?}"
                    );
                }
            }
        }
    }
    let unchanged = PathBuf::from("./a//b/");
    let address = bytes(&unchanged).as_ptr();
    let result = path::normalize(Cow::Owned(unchanged), Path::new("/base"), &mut session)
        .unwrap()
        .unwrap();
    assert_eq!(bytes(&result).as_ptr(), address);
    session.finish(Ok(())).unwrap();
}

#[test]
fn repository_paths_preserve_worktree_submodule_and_bare_transformations() {
    use repository::Kind;
    let cases = [
        (
            "/main/.git",
            Kind::WorkTree {
                linked_git_dir: None,
            },
            "/main/.git",
            Some("/main"),
        ),
        (
            ".git",
            Kind::WorkTree {
                linked_git_dir: None,
            },
            "./.git",
            Some("."),
        ),
        (
            "/wt/.git",
            Kind::WorkTree {
                linked_git_dir: Some("/main/.git/worktrees/wt".into()),
            },
            "/main/.git/worktrees/wt",
            Some("/wt"),
        ),
        (
            "/main/.git/worktrees/wt",
            Kind::WorkTreeGitDir {
                work_dir: "/wt".into(),
            },
            "/main/.git/worktrees/wt",
            Some("/wt"),
        ),
        (
            "/main/sub/.git",
            Kind::Submodule {
                git_dir: "/main/sub/../../repo/.git/modules/sub".into(),
            },
            "/repo/.git/modules/sub",
            Some("/main/sub"),
        ),
        (
            "/main/sub/..",
            Kind::Submodule {
                git_dir: "/main/.git/modules/sub".into(),
            },
            "/main/.git/modules/sub",
            Some("/main"),
        ),
        (
            "/main/.git/modules/sub",
            Kind::SubmoduleGitDir,
            "/main/.git/modules/sub",
            None,
        ),
        ("/bare.git/", Kind::PossiblyBare, "/bare.git/", None),
    ];
    let mut account = Account;
    let mut session = Session::new(&mut account);
    for (dir, kind, expected_git, expected_work) in cases {
        let result =
            repository::Path::from_dot_git_dir(dir.into(), kind, Path::new("/main"), &mut session)
                .unwrap()
                .unwrap();
        let (git, work) = result.into_parts(&mut session).unwrap();
        assert_eq!(bytes(&git), expected_git.as_bytes());
        assert_eq!(work.as_deref().map(bytes), expected_work.map(str::as_bytes));
    }
    assert!(repository::Path::from_dot_git_dir(
        "/wt/.git".into(),
        Kind::Submodule {
            git_dir: "/../bad".into()
        },
        Path::new("/main"),
        &mut session
    )
    .unwrap()
    .is_none());
    assert!(repository::Path::from_dot_git_dir(
        "/..".into(),
        Kind::WorkTree {
            linked_git_dir: None
        },
        Path::new("/main"),
        &mut session
    )
    .unwrap()
    .is_none());
    for (input, expected) in [
        ("/a/.git", Some(path::Kind::Common)),
        ("/a/.git/modules/sub", Some(path::Kind::Submodule)),
        ("/a/.git/worktrees/wt", Some(path::Kind::LinkedWorktree)),
        (
            "/a/.git/modules/sub/.git/worktrees/wt",
            Some(path::Kind::LinkedWorktree),
        ),
        ("modules/sub", Some(path::Kind::Submodule)),
        ("/a.git", None),
        ("/a/.git/unrelated/sub", None),
    ] {
        assert_eq!(
            path::repository_kind(Path::new(input), &mut session).unwrap(),
            expected
        );
    }
    session.finish(Ok(())).unwrap();
}

#[test]
fn refused_growth_stops_the_original_session() {
    struct Refuse {
        requests: usize,
    }
    impl document::Admission for Refuse {
        type Error = ();
        fn admit(&mut self, request: document::Request) -> Result<(), ()> {
            self.requests += 1;
            if matches!(request, document::Request::Allocation { .. }) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    let mut account = Refuse { requests: 0 };
    let mut session = Session::new(&mut account);
    assert!(path::normalize(
        Cow::Borrowed(Path::new("../file")),
        Path::new("/base"),
        &mut session
    )
    .is_err());
    assert!(path::normalize(
        Cow::Borrowed(Path::new("unchanged")),
        Path::new("/base"),
        &mut session
    )
    .is_err());
    assert!(session.finish(Ok(())).is_err());
    assert!(account.requests > 0);
}
