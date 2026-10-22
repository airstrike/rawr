// Copyright the ripgrep contributors. MIT OR Unlicense.
//! Pinned ignore root stripping and globset candidate preparation on Unix.
use crate::input::membership::manifest::{load::path, session::Session, Step};
use rawr_toml::document::Admission;
use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

pub(super) struct Root(PathBuf);
impl Root {
    /// Clone an already prepared root without repeating its preparation.
    pub(super) fn copy<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Self> {
        Ok(Self(path::copy(&self.0, session)?))
    }

    pub(super) fn prepare<A: Admission>(root: &Path, session: &mut Session<'_, A>) -> Step<Self> {
        let root = strip(b"./", root, session)?.unwrap_or(root);
        Ok(Self(path::copy(root, session)?))
    }
    pub(super) fn strip<'a, A: Admission>(
        &self,
        input: &'a Path,
        session: &mut Session<'_, A>,
    ) -> Step<&'a Path> {
        let mut input = strip(b"./", input, session)?.unwrap_or(input);
        // Path equality follows native lexical component semantics, unlike the
        // byte-prefix operation below. Its traversal is charged before use.
        session.items(self.0.as_os_str().len())?;
        if self.0 != Path::new(".") {
            let bytes = input.as_os_str().as_bytes();
            session.items(bytes.len())?;
            if bytes.contains(&b'/')
                && let Some(relative) = strip(self.0.as_os_str().as_bytes(), input, session)?
            {
                input = strip(b"/", relative, session)?.unwrap_or(relative);
            }
        }
        Ok(input)
    }
}
fn strip<'a, A: Admission>(
    prefix: &[u8],
    path: &'a Path,
    session: &mut Session<'_, A>,
) -> Step<Option<&'a Path>> {
    session.check()?;
    let bytes = path.as_os_str().as_bytes();
    session.items(1)?;
    if prefix.len() > bytes.len() {
        return Ok(None);
    }
    session.items(prefix.len())?;
    Ok(bytes
        .strip_prefix(prefix)
        .map(|suffix| Path::new(OsStr::from_bytes(suffix))))
}

/// Borrowed Unix Candidate::new path, basename and globset extension. No
/// filesystem observation or path normalization is performed by this stage.
pub(super) struct Candidate<'a> {
    pub(super) path: &'a [u8],
    pub(super) basename: &'a [u8],
    pub(super) extension: &'a [u8],
}
impl<'a> Candidate<'a> {
    pub(super) fn prepare<A: Admission>(
        path: &'a Path,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        session.items(1)?;
        let path = path.as_os_str().as_bytes();
        let basename = if path.is_empty() {
            &[][..]
        } else {
            session.items(path.len())?;
            let start = path
                .iter()
                .rposition(|&byte| byte == b'/')
                .map_or(0, |index| index + 1);
            let basename = &path[start..];
            session.items(basename.len().min(2))?;
            if basename == b".." {
                &[][..]
            } else {
                basename
            }
        };
        let extension = if basename.is_empty() {
            &[][..]
        } else {
            session.items(basename.len())?;
            basename
                .iter()
                .rposition(|&byte| byte == b'.')
                .map_or(&[][..], |index| &basename[index..])
        };
        Ok(Self {
            path,
            basename,
            extension,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    #[test]
    fn root_stripping_preserves_byte_prefix_and_dot_rules() {
        for (root, input, expected) in [
            ("./root", "./root/file.rs", "file.rs"),
            ("root", "rooted/file", "ed/file"),
            ("root", "rooted", "rooted"),
            (".", ".hidden/file", ".hidden/file"),
            ("././", ".hidden/file", ".hidden/file"),
            ("/p", "/p//file", "/file"),
            ("./", "./a/b", "a/b"),
            ("root", "elsewhere/file", "elsewhere/file"),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let root = Root::prepare(Path::new(root), &mut session).unwrap();
            assert_eq!(
                root.strip(Path::new(input), &mut session)
                    .unwrap()
                    .as_os_str()
                    .as_bytes(),
                expected.as_bytes()
            );
        }
    }
    #[test]
    fn candidate_preserves_native_bytes_and_glob_extensions() {
        for (input, basename, extension) in [
            (&b"foo/bar.rs"[..], &b"bar.rs"[..], &b".rs"[..]),
            (b".rs", b".rs", b".rs"),
            (b"foo/..", b"", b""),
            (b"foo/.", b".", b"."),
            (b"foo/", b"", b""),
            (b"", b"", b""),
            (b"foo\\bar.\xff", b"foo\\bar.\xff", b".\xff"),
        ] {
            let candidate = Candidate::prepare(
                Path::new(OsStr::from_bytes(input)),
                &mut Session::new(&mut Account),
            )
            .unwrap();
            assert_eq!(
                (candidate.path, candidate.basename, candidate.extension),
                (input, basename, extension)
            );
            assert_eq!(candidate.path.as_ptr(), input.as_ptr());
        }
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let index = self.calls;
            self.calls += 1;
            if index == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    fn prepare<A: Admission>(session: &mut Session<'_, A>) -> Step<()> {
        let root = Root::prepare(Path::new("./root"), session)?;
        let path = root.strip(Path::new("./root/dir/file.rs"), session)?;
        Candidate::prepare(path, session)?;
        Ok(())
    }
    #[test]
    fn refusal_prevents_later_path_preparation() {
        let mut complete = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        prepare(&mut Session::new(&mut complete)).unwrap();
        for refuse in 0..complete.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(prepare(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}
