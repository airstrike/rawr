//! Which kept expansion sources stay (#2152).
//!
//! Each source's first line names the artifact of its macro's crate it was
//! expanded against. A build that makes another artifact for that crate
//! retires the source (`retire`): a derive's is expanded again once loading
//! the program asks for it, as an item an edit changed is, and a call's is
//! expanded again in its place, from the call its first line names. Each
//! source a check reads is named in the [`Named`] its owner lends the
//! check, as it is read or as a lowering kept from an earlier check that
//! read it is reused. The owner holds them across its rounds and saves,
//! and once a check passes, each source no check named since the last
//! prune is removed (`prune`): a call an edit took out of the program, or
//! an item it renamed, leaves nothing behind.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The kept expansion sources an owner's checks read since their
/// project's last prune.
#[derive(Debug, Default)]
pub struct Named(BTreeSet<PathBuf>);

impl Named {
    /// No source named yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Note that a check read the kept expansion source at `path`.
    pub fn name(&mut self, path: &Path) {
        if !self.0.contains(path) {
            self.0.insert(path.to_path_buf());
        }
    }

    /// Take each source `other` names, which is left naming none.
    pub fn append(&mut self, other: &mut Self) {
        self.0.append(&mut other.0);
    }

    /// Whether a check named the source at `path`.
    #[must_use]
    pub fn names(&self, path: &Path) -> bool {
        self.0.contains(path)
    }
}

/// Remove each kept expansion source of the project at `project` that no
/// check named in `named`, which then names none of the project's. A
/// check that passed read every source the program needs, so what it
/// didn't name nothing reaches.
pub fn prune(project: &Path, named: &mut Named) -> io::Result<()> {
    let directory = super::directory(project);
    let (under, others) = std::mem::take(&mut named.0)
        .into_iter()
        .partition::<BTreeSet<_>, _>(|path| path.starts_with(&directory));
    named.0 = others;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if source(&path) && !under.contains(&path) {
            remove(&path)?;
        }
    }
    Ok(())
}

/// The path and bytes of the basis the kept sources of the project at
/// `project` are current against, when it keeps one. A kept expansion is
/// its call's or item's text expanded against the artifact of its macro's
/// crate the basis names, so the program's snapshot holds the basis beside
/// the sources that hold the calls (#2156): a build of the macro's crate
/// that changes an expansion keys the next check apart from a check of the
/// old one.
#[must_use]
pub fn basis(project: &Path) -> Option<(PathBuf, Vec<u8>)> {
    let path = super::directory(project).join(super::BASIS);
    let bytes = fs::read(&path).ok()?;
    Some((path, bytes))
}

/// What [`retire`] retired.
#[cfg(feature = "rustc-driver")]
#[derive(Debug, Default)]
pub struct Retired {
    /// Whether a derive's source was retired: a load that read it loads
    /// the program again, which asks for its expansion.
    pub derives: bool,
    /// Each call whose source was retired, which rustc expands again.
    pub calls: Vec<super::call::Call>,
    /// Whether another basis was kept ([`basis`]): a snapshot taken before
    /// holds the one it replaced.
    pub basis: bool,
}

/// Retire each kept source of the project at `project` that was expanded
/// against another artifact of its macro's crate than `basis` names, and
/// keep `basis` as the one the kept sources are current against.
#[cfg(feature = "rustc-driver")]
pub fn retire(project: &Path, basis: &super::Basis) -> io::Result<Retired> {
    let directory = super::directory(project);
    let mut retired = Retired::default();
    if !directory.is_dir() {
        return Ok(retired);
    }
    // The sources are current against the basis kept beside them.
    if super::Basis::kept(&directory).is_some_and(|kept| kept.externs == basis.externs) {
        return Ok(retired);
    }
    let superseded =
        |path: &str, artifact: &str| basis.artifact(path).is_some_and(|now| now != artifact);
    for entry in fs::read_dir(&directory)? {
        let path = entry?.path();
        if !source(&path) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some((call, artifact)) = super::call::built(&text) {
            if superseded(&call.path, artifact) {
                remove(&path)?;
                retired.calls.push(call);
            }
        } else if let Some((resolved, artifact)) = super::built(&text)
            && superseded(resolved, artifact)
        {
            remove(&path)?;
            retired.derives = true;
        }
    }
    basis.keep(&directory)?;
    retired.basis = true;
    Ok(retired)
}

/// Whether `path` is a kept expansion source: a derive's or a call's, never
/// the basis or a source another process is still writing.
fn source(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "rs")
}

/// Remove the file at `path`, which another process's prune may have
/// removed first.
fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    fn project(name: &str) -> PathBuf {
        let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-expand-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(super::super::directory(&path)).unwrap();
        path
    }

    #[test]
    fn a_passing_check_keeps_only_the_sources_it_named() {
        let project = project("prune");
        let directory = super::super::directory(&project);
        let named = directory.join("call-a.rs");
        let dropped = directory.join("call-b.rs");
        let basis = directory.join("basis.json");
        for path in [&named, &dropped, &basis] {
            fs::write(path, "3\n").unwrap();
        }
        let mut names = super::Named::default();
        names.name(&named);
        super::prune(&project, &mut names).unwrap();
        assert!(named.is_file());
        assert!(!dropped.exists());
        assert!(basis.is_file());
        // A prune forgets what it was named: the next check names again.
        assert!(!names.names(&named));
        super::prune(&project, &mut names).unwrap();
        assert!(!named.exists());
        let _ = fs::remove_dir_all(&project);
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn a_build_of_another_artifact_retires_the_sources_of_its_crate() {
        use super::super::call::{Call, Delimiter, Place};
        let project = project("retire");
        let directory = super::super::directory(&project);
        let basis = |numerals: &str| super::super::Basis {
            selected_target: "aarch64-apple-darwin".into(),
            dependency_directories: Box::default(),
            externs: Box::new([
                (
                    "numerals".into(),
                    "/deps/libnumerals.dylib".into(),
                    numerals.into(),
                ),
                ("serde".into(), "/deps/libserde.rmeta".into(), "s1".into()),
            ]),
        };
        let called = Call {
            path: "numerals::double".to_owned(),
            delimiter: Delimiter::Parenthesis,
            input: "3".to_owned(),
            place: Place::Expression,
        };
        let call = directory.join("call-a.rs");
        let derive = directory.join("b.rs");
        let header = serde_json::to_string(&called).unwrap();
        fs::write(&call, format!("// call a1 {header}\n((3) * 2)\n")).unwrap();
        fs::write(
            &derive,
            super::super::source_text("serde::Serialize", "struct A;", "s1", &[]),
        )
        .unwrap();
        let retired = super::retire(&project, &basis("a1")).unwrap();
        assert!(!retired.derives && retired.calls.is_empty());
        assert!(call.is_file());
        // The macro's crate built again: its call is expanded again, and
        // another crate's derive stays.
        let retired = super::retire(&project, &basis("a2")).unwrap();
        assert_eq!(retired.calls, [called]);
        assert!(!retired.derives);
        assert!(!call.exists());
        assert!(derive.is_file());
        let _ = fs::remove_dir_all(&project);
    }
}
