use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cargo;
use crate::node::{Kind, Value};

use super::{Context, Error};

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct ValueFixture;

impl Value for ValueFixture {
    const KIND: Kind = Kind::Api;
    const SCHEMA: u32 = 1;

    fn encode(&self, _limit: usize) -> Result<Box<[u8]>, crate::node::Error> {
        Ok(Box::default())
    }

    fn decode(_bytes: &[u8], _limit: usize) -> Result<Self, crate::node::Error> {
        Ok(Self)
    }
}

#[test]
fn explicit_and_home_fallback_cargo_homes_resolve_only_to_canonical_v1_stores() {
    let fixture = Fixture::new("cargo-home");
    let explicit = cargo::config::Environment::from_pairs([(
        "CARGO_HOME",
        fixture
            .cargo_home
            .to_str()
            .expect("the fixture path is Unicode"),
    )])
    .expect("the explicit environment seals");
    let context = Context::capture(
        &fixture.project,
        explicit,
        "aarch64-apple-darwin",
        &cargo::Limits::standard(),
    )
    .expect("the explicit context seals");
    assert_eq!(context.project(), fixture.project);
    assert_eq!(context.cargo_home(), fixture.cargo_home);
    assert_eq!(context.target(), "aarch64-apple-darwin");
    assert_eq!(
        context.open_store().expect("the store opens").root(),
        fixture.project.join("target/rawr/store"),
        "the store lives in the project's target directory"
    );

    let home = fixture.root.join("home");
    let fallback = home.join(".cargo");
    fs::create_dir(&home).expect("the fallback home exists");
    fs::create_dir(&fallback).expect("the fallback Cargo home exists");
    let environment = cargo::config::Environment::from_pairs([(
        "HOME",
        home.to_str().expect("the fixture path is Unicode"),
    )])
    .expect("the fallback environment seals");
    let context = Context::capture(
        &fixture.project,
        environment,
        "aarch64-apple-darwin",
        &cargo::Limits::standard(),
    )
    .expect("the fallback context seals");
    assert_eq!(
        context.cargo_home(),
        fs::canonicalize(fallback).expect("the fallback Cargo home canonicalizes")
    );
}

#[test]
fn missing_empty_relative_and_symlinked_cargo_home_authority_rejects_before_store_creation() {
    let fixture = Fixture::new("cargo-home-rejection");
    assert!(matches!(
        Context::capture(
            &fixture.project,
            cargo::config::Environment::empty(),
            "target",
            &cargo::Limits::standard(),
        ),
        Err(Error::MissingCargoHome)
    ));
    for rejected in ["", "relative/cargo-home"] {
        let environment = cargo::config::Environment::from_pairs([("CARGO_HOME", rejected)])
            .expect("the explicit environment shape seals before path validation");
        assert!(matches!(
            Context::capture(
                &fixture.project,
                environment,
                "target",
                &cargo::Limits::standard(),
            ),
            Err(Error::Environment(crate::Error::Path { .. }))
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let link = fixture.root.join("cargo-home-link");
        symlink(&fixture.cargo_home, &link).expect("the Cargo-home symlink exists");
        let environment = cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            link.to_str().expect("the fixture path is Unicode"),
        )])
        .expect("the symlink environment seals");
        assert!(matches!(
            Context::capture(
                &fixture.project,
                environment,
                "target",
                &cargo::Limits::standard(),
            ),
            Err(Error::Path {
                field: "Cargo home",
                ..
            })
        ));
    }
    assert!(!fixture.cargo_home.join("rawr").exists());
}

#[test]
fn environment_target_and_limit_drift_change_context_and_direct_action_identity() {
    let fixture = Fixture::new("identity");
    let limits = cargo::Limits::standard();
    let context = |environment, target, limits| {
        Context::capture(&fixture.project, environment, target, limits).expect("the context seals")
    };
    let environment = |value: &str| {
        cargo::config::Environment::from_pairs([
            (
                "CARGO_HOME",
                fixture.cargo_home.to_str().expect("the path is Unicode"),
            ),
            ("RUSTFLAGS", value),
        ])
        .expect("the environment seals")
    };
    let first = context(environment("one"), "target-a", &limits);
    let environment_changed = context(environment("two"), "target-a", &limits);
    let target_changed = context(environment("one"), "target-b", &limits);
    let bounded = cargo::Limits::fixture(
        limits.byte_limit().saturating_sub(1),
        limits.item_limit(),
        1024 * 1024,
    );
    let limits_changed = context(environment("one"), "target-a", &bounded);
    let actions = [
        first
            .direct::<ValueFixture>(b"same semantic input", 64)
            .expect("the action seals")
            .action(),
        environment_changed
            .direct::<ValueFixture>(b"same semantic input", 64)
            .expect("the action seals")
            .action(),
        target_changed
            .direct::<ValueFixture>(b"same semantic input", 64)
            .expect("the action seals")
            .action(),
        limits_changed
            .direct::<ValueFixture>(b"same semantic input", 64)
            .expect("the action seals")
            .action(),
    ];
    assert_ne!(actions[0], actions[1]);
    assert_ne!(actions[0], actions[2]);
    assert_ne!(actions[0], actions[3]);
    assert_ne!(first.identity(), environment_changed.identity());
}

#[test]
fn only_build_relevant_environment_changes_context_identity() {
    let fixture = Fixture::new("relevant");
    let home = fixture.cargo_home.to_str().expect("the path is Unicode");
    let identity = |extra: &[(&str, &str)]| {
        let environment = cargo::config::Environment::from_pairs(
            [("CARGO_HOME", home)]
                .into_iter()
                .chain(extra.iter().copied()),
        )
        .expect("the environment seals");
        Context::capture(
            &fixture.project,
            environment,
            "target-a",
            &cargo::Limits::standard(),
        )
        .expect("the context seals")
        .identity()
    };
    let base = identity(&[]);
    assert_eq!(
        base,
        identity(&[
            ("RAWR_UNRELATED_PROBE", "1"),
            ("RUST_BACKTRACE", "1"),
            ("RUST_LOG", "debug"),
            ("CARGO_TERM_COLOR", "always"),
        ])
    );
    assert_ne!(base, identity(&[("RUSTFLAGS", "-Copt-level=1")]));
    assert_ne!(base, identity(&[("PATH", "/x")]));
}

/// The context identity under one PATH built from fixture tool directories.
fn path_identity(fixture: &Fixture, directories: &[&str]) -> [u8; 32] {
    let path = directories
        .iter()
        .map(|directory| fixture.root.join(directory).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    let environment = cargo::config::Environment::from_pairs([
        (
            "CARGO_HOME",
            fixture.cargo_home.to_str().expect("the path is Unicode"),
        ),
        ("PATH", path.as_str()),
    ])
    .expect("the environment seals");
    Context::capture(
        &fixture.project,
        environment,
        "target-a",
        &cargo::Limits::standard(),
    )
    .expect("the context seals")
    .identity()
}

fn tool(fixture: &Fixture, directory: &str, name: &str, bytes: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    let directory = fixture.root.join(directory);
    fs::create_dir_all(&directory).expect("the tool directory exists");
    fs::write(directory.join(name), bytes).expect("the tool is written");
    fs::set_permissions(directory.join(name), fs::Permissions::from_mode(0o755))
        .expect("the tool is executable");
}

#[test]
fn reordered_path_resolving_the_same_tools_shares_one_identity() {
    let fixture = Fixture::new("path-same");
    tool(&fixture, "a", "rustc", "rustc");
    tool(&fixture, "b", "cc", "cc");
    fs::create_dir(fixture.root.join("empty")).expect("the empty directory exists");
    assert_eq!(
        path_identity(&fixture, &["a", "b"]),
        path_identity(&fixture, &["empty", "b", "a", "a"])
    );
}

#[test]
fn path_resolving_a_different_rustc_changes_identity() {
    let fixture = Fixture::new("path-different");
    tool(&fixture, "a", "rustc", "rustc one");
    tool(&fixture, "b", "rustc", "rustc two");
    assert_ne!(
        path_identity(&fixture, &["a", "b"]),
        path_identity(&fixture, &["b", "a"])
    );
}

#[cfg(unix)]
#[test]
fn symlinked_store_component_rejects_without_following_it() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("store-symlink");
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).expect("the outside directory exists");
    let context = fixture.context();
    symlink(&outside, context.rawr().join("store")).expect("the store component symlink exists");
    assert!(matches!(
        context.open_store(),
        Err(crate::node::Error::Shape(_))
    ));
    assert!(fs::read_dir(outside)
        .expect("the outside directory remains readable")
        .next()
        .is_none());
}

struct Fixture {
    root: PathBuf,
    project: PathBuf,
    cargo_home: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = temporary(label);
        let project = root.join("project");
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&root).expect("the fixture root exists");
        fs::create_dir(&project).expect("the project exists");
        fs::create_dir(&cargo_home).expect("the Cargo home exists");
        Self {
            root,
            project: fs::canonicalize(project).expect("the project canonicalizes"),
            cargo_home: fs::canonicalize(cargo_home).expect("the Cargo home canonicalizes"),
        }
    }

    fn context(&self) -> Context {
        let environment = cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            self.cargo_home
                .to_str()
                .expect("the fixture path is Unicode"),
        )])
        .expect("the environment seals");
        Context::capture(
            &self.project,
            environment,
            "target",
            &cargo::Limits::standard(),
        )
        .expect("the context seals")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn temporary(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rawr-context-{label}-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ))
}
