#![cfg(not(feature = "no_std"))]

use rawr::project::{self, Target};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Self {
        let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rawr-project-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn file(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn source(temp: &Temp, root: &str, contents: &str) -> PathBuf {
    temp.file(&format!("{root}/src/main.rawr"), contents)
}

fn project(target: Target) -> project::Project {
    match target {
        Target::Project(project) => project,
        Target::File(file) => panic!("expected project, got file `{}`", file.path().display()),
    }
}

#[test]
fn explicit_source_file_never_ascends() {
    let temp = Temp::new("explicit-file");
    source(&temp, "outer", "outer");
    temp.file("outer/rawr.toml", "[project]\n");
    let explicit = temp.file("outer/examples/counter.rawr", "explicit");

    let target = Target::discover(&explicit).unwrap();
    let Target::File(file) = target else {
        panic!("explicit source was promoted to a project");
    };

    assert_eq!(file.path(), fs::canonicalize(explicit).unwrap());
    assert_eq!(file.text(), "explicit");
}

#[test]
fn exact_zero_config_directory_is_a_project() {
    let temp = Temp::new("exact-zero-config");
    source(&temp, "counter", "42");
    let root = temp.path().join("counter");

    let discovered = project(Target::discover(&root).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(root).unwrap());
    assert_eq!(discovered.source().text(), "42");
    assert!(discovered.config().is_none());
}

#[test]
fn nested_directory_finds_one_zero_config_project() {
    let temp = Temp::new("nested-zero-config");
    source(&temp, "counter", "nested");
    let nested = temp.directory("counter/src/components/deep");

    let discovered = project(Target::discover(nested).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(temp.path().join("counter")).unwrap());
    assert_eq!(discovered.source().text(), "nested");
}

#[test]
fn manifest_project_retains_readable_config() {
    let temp = Temp::new("manifest");
    source(&temp, "counter", "configured");
    let manifest = temp.file("counter/rawr.toml", "[application]\nname = \"counter\"\n");

    let discovered = project(Target::discover(temp.path().join("counter")).unwrap());
    let config = discovered.config().expect("manifest is retained");

    assert_eq!(config.path(), fs::canonicalize(manifest).unwrap());
    assert_eq!(config.text(), "[application]\nname = \"counter\"\n");
}

#[test]
fn explicit_manifest_selects_its_exact_project() {
    let temp = Temp::new("explicit-manifest");
    source(&temp, "outer", "outer");
    source(&temp, "outer/inner", "inner");
    let manifest = temp.file("outer/rawr.toml", "[application]\n");

    let discovered = project(Target::discover(manifest).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(temp.path().join("outer")).unwrap());
    assert_eq!(discovered.source().text(), "outer");
}

#[test]
fn exact_candidate_directory_wins_over_an_outer_project() {
    let temp = Temp::new("exact-wins");
    source(&temp, "outer", "outer");
    source(&temp, "outer/inner", "inner");
    let inner = temp.path().join("outer/inner");

    let discovered = project(Target::discover(&inner).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(inner).unwrap());
    assert_eq!(discovered.source().text(), "inner");
}

#[test]
fn nested_zero_config_projects_are_ambiguous() {
    let temp = Temp::new("ambiguous");
    source(&temp, "outer", "outer");
    source(&temp, "outer/inner", "inner");
    let start = temp.directory("outer/inner/components/deep");

    let error = Target::discover(&start).unwrap_err();
    let project::Error::Ambiguous { start: actual_start, candidates } = error else {
        panic!("expected ambiguity, got {error}");
    };

    assert_eq!(actual_start, fs::canonicalize(start).unwrap());
    assert_eq!(candidates.as_ref(), [fs::canonicalize(temp.path().join("outer/inner")).unwrap(), fs::canonicalize(temp.path().join("outer")).unwrap(),]);
}

#[test]
fn ancestor_manifest_joins_lower_candidate_ambiguity() {
    let temp = Temp::new("manifest-ambiguity");
    source(&temp, "outer", "outer");
    temp.file("outer/rawr.toml", "[application]\n");
    source(&temp, "outer/inner", "inner");
    let start = temp.directory("outer/inner/components");

    let error = Target::discover(start).unwrap_err();
    let project::Error::Ambiguous { candidates, .. } = error else {
        panic!("expected ambiguity, got {error}");
    };

    assert_eq!(candidates.as_ref(), [fs::canonicalize(temp.path().join("outer/inner")).unwrap(), fs::canonicalize(temp.path().join("outer")).unwrap(),]);
}

#[test]
fn malformed_manifest_is_a_hard_boundary() {
    let temp = Temp::new("manifest-boundary");
    source(&temp, "outer", "must not be selected");
    temp.file("outer/project/rawr.toml", "[application]\n");
    let start = temp.directory("outer/project/components");

    let error = Target::discover(start).unwrap_err();

    assert!(matches!(error, project::Error::MissingSource { .. }));
    assert!(error.to_string().contains("marks this directory as a project boundary"));
}

#[test]
fn vcs_boundary_stops_ancestor_search_inclusively() {
    let temp = Temp::new("vcs-boundary");
    source(&temp, "outer", "must not be selected");
    temp.directory("outer/repository/.git");
    let start = temp.directory("outer/repository/components/deep");

    let error = Target::discover(&start).unwrap_err();
    let project::Error::NotFound { start: actual_start, boundary } = error else {
        panic!("expected not found, got {error}");
    };

    assert_eq!(actual_start, fs::canonicalize(start).unwrap());
    assert_eq!(boundary, fs::canonicalize(temp.path().join("outer/repository")).unwrap());
}

#[test]
fn missing_input_has_a_positioned_error() {
    let temp = Temp::new("missing-input");
    let missing = temp.path().join("does-not-exist");

    let error = Target::discover(&missing).unwrap_err();

    assert!(matches!(
        error,
        project::Error::InputNotFound { ref path, .. } if path == &missing
    ));
    assert!(error.to_string().contains("the path does not exist"));
}

#[test]
fn invalid_utf8_source_is_unreadable() {
    let temp = Temp::new("invalid-utf8");
    let expected_path = temp.file("counter/src/main.rawr", [0xff, 0xfe]);

    let error = Target::discover(temp.path().join("counter")).unwrap_err();

    assert!(matches!(
        error,
        project::Error::Unreadable {
            resource: project::Resource::Source,
            ref path,
            ..
        } if path == &fs::canonicalize(expected_path).unwrap()
    ));
}

#[test]
fn unsupported_explicit_file_is_actionable() {
    let temp = Temp::new("unsupported");
    let path = temp.file("notes.txt", "not Rawr");

    let error = Target::discover(path).unwrap_err();

    assert!(matches!(error, project::Error::UnsupportedInput { .. }));
    assert!(error.to_string().contains("`.rawr` or `.rs` source file"));
}

#[cfg(unix)]
#[test]
fn project_source_symlink_cannot_escape_root() {
    use std::os::unix::fs::symlink;

    let temp = Temp::new("source-escape");
    let outside = temp.file("outside.rawr", "outside");
    temp.directory("counter/src");
    symlink(outside, temp.path().join("counter/src/main.rawr")).unwrap();

    let error = Target::discover(temp.path().join("counter")).unwrap_err();

    assert!(matches!(error, project::Error::EscapesRoot { resource: project::Resource::Source, .. }));
}

#[cfg(unix)]
#[test]
fn project_config_symlink_cannot_escape_root() {
    use std::os::unix::fs::symlink;

    let temp = Temp::new("config-escape");
    source(&temp, "counter", "counter");
    let outside = temp.file("outside/rawr.toml", "[outside]\n");
    symlink(outside, temp.path().join("counter/rawr.toml")).unwrap();

    let error = Target::discover(temp.path().join("counter")).unwrap_err();

    assert!(matches!(error, project::Error::EscapesRoot { resource: project::Resource::Config, .. }));
}

#[cfg(unix)]
#[test]
fn explicit_source_symlink_uses_its_canonical_target() {
    use std::os::unix::fs::symlink;

    let temp = Temp::new("explicit-symlink");
    let target = temp.file("sources/actual.rawr", "actual");
    let link = temp.path().join("selected.rawr");
    symlink(&target, &link).unwrap();

    let discovered = Target::discover(link).unwrap();
    let Target::File(file) = discovered else {
        panic!("explicit source symlink did not remain a file target");
    };

    assert_eq!(file.path(), fs::canonicalize(target).unwrap());
    assert_eq!(file.text(), "actual");
}
