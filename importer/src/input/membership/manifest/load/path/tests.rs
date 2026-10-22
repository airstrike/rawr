use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use rawr_toml::document;

use crate::input::membership::manifest::{reference, session};

use super::{diff, join, normalize, normalize_text, rebase, Invalid};

mod resources;

#[allow(clippy::redundant_guards, reason = "unchanged upstream source")]
#[path = "reference.rs"]
mod pinned;

fn bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

const PATHS: &[&[u8]] = &[
    b"",
    b".",
    b"..",
    b"/",
    b"//",
    b"///",
    b"a",
    b"a/",
    b"a//b",
    b"a/./b/",
    b"./a/..",
    b"a/../..",
    b"../../a",
    b"a/../../b",
    b"/a/../../b",
    b"//a///b//",
    b"../base",
    b"a/../base",
    b"././",
    b"/..",
    b"\\windows\\path",
    b"C:\\path",
    b"a\0b",
    b"\xff",
    b"/\xff/old",
    b"/\xff",
    b"./a\xff/../b",
    "λ/../β".as_bytes(),
];

#[test]
fn native_join_and_normalization_match_pinned_raw_spelling() {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    for &left in PATHS {
        let left = Path::new(OsStr::from_bytes(left));
        let actual = normalize(left, &mut session).unwrap();
        assert_eq!(
            bytes(&actual),
            bytes(&pinned::cargo::normalize_path(left)),
            "{left:?}"
        );
        for &right in PATHS {
            let right = Path::new(OsStr::from_bytes(right));
            let actual = join(left, right, &mut session).unwrap();
            assert_eq!(
                bytes(&actual),
                bytes(&left.join(right)),
                "join {left:?} {right:?}"
            );
            let actual = normalize(&actual, &mut session).unwrap();
            let expected = pinned::cargo::normalize_path(&left.join(right));
            assert_eq!(
                bytes(&actual),
                bytes(&expected),
                "normalize join {left:?} {right:?}"
            );
        }
    }
    session.finish(Ok(())).unwrap();
}

#[test]
fn all_pathdiff_component_branches_match_unchanged_reference() {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    for &left in PATHS {
        let left = Path::new(OsStr::from_bytes(left));
        for &right in PATHS {
            let right = Path::new(OsStr::from_bytes(right));
            let actual = diff(left, right, &mut session).unwrap();
            let expected = pinned::diff::diff_paths(left, right);
            assert_eq!(
                actual.as_deref().map(bytes),
                expected.as_deref().map(bytes),
                "{left:?} relative to {right:?}"
            );
        }
    }
    session.finish(Ok(())).unwrap();
}

#[test]
fn complete_rebase_order_matches_cargo_and_rejects_only_unrepresentable_result() {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    for &old in PATHS {
        let old = Path::new(OsStr::from_bytes(old));
        for &new in PATHS {
            let new = Path::new(OsStr::from_bytes(new));
            for relative in ["file", "../LICENSE", "a/../../b", "/absolute", "", "λ"] {
                let expected = match pinned::diff::diff_paths(
                    pinned::cargo::normalize_path(&old.join(relative)),
                    new,
                ) {
                    None => Err(Invalid::Relative),
                    Some(path) => path
                        .into_os_string()
                        .into_string()
                        .map_err(|_| Invalid::Utf8),
                };
                assert_eq!(
                    rebase(old, new, relative, &mut session).unwrap(),
                    expected,
                    "{old:?} {new:?} {relative:?}"
                );
            }
        }
    }
    assert_eq!(
        rebase(
            Path::new(OsStr::from_bytes(b"/\xff/old")),
            Path::new(OsStr::from_bytes(b"/\xff")),
            "file",
            &mut session
        )
        .unwrap(),
        Ok("old/file".to_owned())
    );
    session.finish(Ok(())).unwrap();
}

#[test]
fn string_and_native_ownership_transfers_do_not_allocate_or_copy() {
    let mut account = reference::Account;
    let mut session = session::Session::new(&mut account);
    let mut owned = super::allocate(4, &mut session).unwrap();
    owned.extend_from_slice(b"path");
    let pointer = owned.as_ptr();
    let path = super::native(owned);
    assert_eq!(path.as_os_str().as_bytes().as_ptr(), pointer);
    let text = String::from_utf8(path.into_os_string().into_vec()).unwrap();
    assert_eq!(text.as_ptr(), pointer);
    for text in [
        "",
        ".",
        "build.rs",
        "./build.rs",
        "a/../build.rs",
        "../λ.rs",
        "/../a",
    ] {
        let actual = normalize_text(text, &mut session).unwrap();
        assert_eq!(
            actual.as_bytes(),
            pinned::cargo::normalize_path(Path::new(text))
                .as_os_str()
                .as_bytes()
        );
    }
    // Native names are data; NUL and invalid UTF-8 do not cause any syscall.
    let native = OsString::from_vec(b"a\0\xff".to_vec());
    assert_eq!(
        bytes(&normalize(Path::new(&native), &mut session).unwrap()),
        b"a\0\xff"
    );
    session.finish(Ok(())).unwrap();
}

fn exercise<A: document::Admission>(
    session: &mut session::Session<'_, A>,
) -> crate::input::membership::manifest::Step<()> {
    join(Path::new("/root/"), Path::new("a//../b/"), session)?;
    normalize(Path::new("../../a/b/c/d/e/f/../../g"), session)?;
    diff(Path::new("a/b/c/d/e/f"), Path::new("other/x/y/z"), session)?;
    normalize_text("a/../build.rs", session)?;
    assert!(rebase(
        Path::new("/root"),
        Path::new("/root/member"),
        "LICENSE",
        session
    )?
    .is_ok());
    assert_eq!(
        rebase(
            Path::new("relative"),
            Path::new("/absolute"),
            "file",
            session
        )?,
        Err(Invalid::Relative)
    );
    assert_eq!(
        rebase(
            Path::new(OsStr::from_bytes(b"/\xff")),
            Path::new("/"),
            "file",
            session
        )?,
        Err(Invalid::Utf8)
    );
    Ok(())
}
