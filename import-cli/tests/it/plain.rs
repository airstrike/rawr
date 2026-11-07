//! A program with one Cargo dependency and no GUI, run by the real
//! `rawr-import` binary. It finds `rawr-import-driver` beside itself, as an
//! install does: build it first with
//! `cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver`.
//! The project sits under Cargo's test directory and keeps its `target/`
//! between runs, as a user's project does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/plain-dependency"
);

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

fn rawr_import(root: &Path, arguments: &[&str]) -> Output {
    let home = crate::support::scratch().join("rawr-home");
    rawr_import_with_home(root, arguments, &home)
}

fn rawr_import_with_home(root: &Path, arguments: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .args(arguments)
        .arg(root)
        .env("RAWR_HOME", home)
        .env("RAWR_RUNTIME_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The pinned nightly's check build directory, shared by every project
/// that uses `home` (D700).
fn nightly_directory(home: &Path) -> PathBuf {
    home.join("nightly")
        .join(rawr_import::cargo::toolchain::CHANNEL)
}

/// Whether the project at `root` ran the nightly check build: its bundle
/// keeps a copy of the harness's metadata from each one.
fn nightly_built(root: &Path) -> bool {
    fn walk(path: &Path) -> bool {
        fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path)
                } else {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("librawr_resolution_harness")
                }
            })
    }
    walk(&root.join("target/rawr"))
}

/// The units the nightly check build compiled or ran since `since`, by
/// Cargo's `<crate>-<hash>` names: Cargo rewrites a unit's fingerprint
/// only when it compiles that unit or runs its build script again.
fn compiled_since(nightly: &Path, since: std::time::SystemTime) -> Vec<String> {
    let mut units = Vec::new();
    let mut pending = vec![nightly.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if directory
                .file_name()
                .is_some_and(|name| name == ".fingerprint")
            {
                let written = fs::read_dir(&path)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|file| {
                        file.metadata()
                            .and_then(|metadata| metadata.modified())
                            .is_ok_and(|modified| modified > since)
                    });
                if written {
                    units.push(entry.file_name().to_string_lossy().into_owned());
                }
            } else {
                pending.push(path);
            }
        }
    }
    units.sort();
    units
}

/// The units in the nightly check build named for `krate`.
fn units_of(nightly: &Path, krate: &str) -> Vec<String> {
    compiled_since(nightly, std::time::SystemTime::UNIX_EPOCH)
        .into_iter()
        .filter(|unit| {
            unit.rsplit_once('-')
                .is_some_and(|(name, hash)| name == krate && hash.len() == 16)
        })
        .collect()
}

/// `rawr_import`, killed and failed after `limit`: a run that waits on
/// itself fails the test instead of hanging it.
fn rawr_import_within(
    root: &Path,
    arguments: &[&str],
    home: &Path,
    limit: std::time::Duration,
) -> Output {
    use std::process::Stdio;
    // Beside the project: several tests name theirs `app`, and they run at
    // once.
    let stdout = root.with_extension("stdout");
    let stderr = root.with_extension("stderr");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .args(arguments)
        .arg(root)
        .env("RAWR_HOME", home)
        .env("RAWR_RUNTIME_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "rawr-import {arguments:?} ran past {limit:?}:\n{}",
                fs::read_to_string(&stderr).unwrap_or_default()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    Output {
        status,
        stdout: fs::read(&stdout).unwrap(),
        stderr: fs::read(&stderr).unwrap(),
    }
}

#[test]
fn a_plain_program_with_a_dependency_runs_in_a_runner_without_iced() {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = crate::support::scratch().join("plain-dependency");
    copy(Path::new(FIXTURE), &root);

    let run = rawr_import(&root, &["run"]);

    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        "parsed: 1.2.3\nmajor: 1\nwidest: 18446744073709551615.0.9223372036854775808\n"
    );
    // The runner links rawr's runtime, compiled once in rawr's home, and
    // compiles no rawr crate in the project (D620).
    let runner = fs::read_to_string(root.join("target/rawr/runner/Cargo.toml")).unwrap();
    assert!(!runner.contains("__rawr_internal"), "{runner}");
    assert!(runner.contains("semver = "), "{runner}");
    assert!(!runner.contains("iced"), "{runner}");
    let main = fs::read_to_string(root.join("target/rawr/runner/src/main.rs")).unwrap();
    assert!(
        main.contains("extern crate rawr as __rawr_internal;"),
        "{main}"
    );
    let script = fs::read_to_string(root.join("target/rawr/runner/build.rs")).unwrap();
    assert!(
        script.contains("cargo:rustc-link-search=crate=") && script.contains("/runtime/"),
        "{script}"
    );
    let compiled = fs::read_dir(root.join("target/debug/deps"))
        .unwrap()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.starts_with("librawr-") || name.starts_with("librawr_import-"))
        .collect::<Vec<_>>();
    assert!(compiled.is_empty(), "{compiled:?}");

    let checked = rawr_import(&root, &["check"]);
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    // `cargo check` runs beside Rawr's check and reports after it.
    assert_eq!(
        text(&checked.stdout),
        format!(
            "Checked {}\ncargo check passed\n",
            root.join("src/main.rs").display()
        )
    );
}

#[test]
fn a_concrete_blanket_application_keeps_its_trait_arguments() {
    let root = crate::support::scratch().join("concrete-trait-arguments");
    let home = crate::support::scratch().join("rawr-home");
    for (path, source) in [
        (
            "Cargo.toml",
            "[workspace]\n[package]\nname='concrete-trait-arguments'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-a={path='a'}\nfacts-c={path='c'}\n",
        ),
        (
            "b/Cargo.toml",
            "[package]\nname='facts-b'\nversion='0.1.0'\nedition='2024'\n",
        ),
        (
            "b/src/lib.rs",
            "pub trait Base<U> { fn base(&self) -> U; }\npub trait LeftMark {}\npub trait RightMark {}\n",
        ),
        (
            "a/Cargo.toml",
            "[package]\nname='facts-a'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-b={path='../b'}\n",
        ),
        (
            "a/src/lib.rs",
            r#"use facts_b::{Base, LeftMark, RightMark};
#[derive(Clone, Copy)] pub struct LeftOnly;
#[derive(Clone, Copy)] pub struct RightOnly;
#[derive(Clone, Copy)] pub struct Both;
impl Base<i64> for LeftOnly { fn base(&self) -> i64 { 101 } }
impl Base<i64> for RightOnly { fn base(&self) -> i64 { 202 } }
impl Base<i64> for Both { fn base(&self) -> i64 { 303 } }
impl LeftMark for LeftOnly {}
impl RightMark for RightOnly {}
impl LeftMark for Both {}
impl RightMark for Both {}
pub fn left() -> LeftOnly { LeftOnly }
pub fn right() -> RightOnly { RightOnly }
pub fn both() -> Both { Both }
"#,
        ),
        (
            "c/Cargo.toml",
            "[package]\nname='facts-c'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-b={path='../b'}\n",
        ),
        (
            "c/src/lib.rs",
            r#"use facts_b::{Base, LeftMark, RightMark};
pub trait Extension<U> { fn extension(&self) -> U; }
impl<T, U> Extension<U> for T where T: Base<U> {
    fn extension(&self) -> U { self.base() }
}
pub trait Left { fn choose(&self) -> i64; }
impl<T: Base<i64> + LeftMark> Left for T { fn choose(&self) -> i64 { 11 } }
pub trait Right { fn choose(&self) -> bool; }
impl<T: Base<i64> + RightMark> Right for T { fn choose(&self) -> bool { true } }
"#,
        ),
        (
            "src/main.rs",
            r#"// rawr
use facts_a::{LeftOnly, RightOnly};
use facts_c::{Extension, Left, Right};
fn main() {
    let scalar: i64 = facts_a::left().extension();
    let left: i64 = facts_a::left().choose();
    let right: bool = facts_a::right().choose();
    Left::choose(&facts_a::left());
    Right::choose(&facts_a::right());
    println!("{scalar} {left} {right}");
}
"#,
        ),
    ] {
        let destination = root.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, source).unwrap();
    }
    let valid = fs::read_to_string(root.join("src/main.rs")).unwrap();
    // Keep the exact extraction, facade and runtime caches through repeated
    // valid -> ambiguous -> valid transitions, including a repeated test run.
    for _ in 0..2 {
        fs::write(root.join("src/main.rs"), &valid).unwrap();
        let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(120));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), "101 11 true\n");

        fs::write(
            root.join("src/main.rs"),
            "// rawr\nuse facts_a::Both;\nuse facts_c::{Left, Right};\nfn main() { let output: i64 = facts_a::both().choose(); println!(\"{output}\"); }\n",
        )
        .unwrap();
        let ambiguous = rawr_import_within(
            &root,
            &["check"],
            &home,
            std::time::Duration::from_secs(120),
        );
        assert!(!ambiguous.status.success());
        assert!(text(&ambiguous.stderr).contains("error[E0034]"));
        assert!(!text(&ambiguous.stderr).contains("symbolic surface identity"));
    }
    fs::write(root.join("src/main.rs"), &valid).unwrap();
    let restored = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(120));
    assert!(
        restored.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&restored.stdout),
        text(&restored.stderr)
    );
    assert_eq!(text(&restored.stdout), "101 11 true\n");
}

/// One method name whose candidate depends on the call's lexical scope:
/// each function imports a different trait in its own block. Rawr proves
/// each call from the crates' intrinsic facts (D650) and the runner calls
/// the native method through a helper that imports only that block's trait.
/// Each crate's facts are kept under its own key: a later run reuses them,
/// and a changed crate gets a new entry alone.
#[test]
fn a_trait_method_resolves_in_each_lexical_scope_through_a_proved_application() {
    let base = crate::support::scratch().join("lexical-applications");
    let home = base.join("home");
    let crates = base.join("crates");
    let _ = fs::remove_dir_all(&base);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../importer/tests/fixtures");
    for name in ["a", "b", "c"] {
        copy(
            &fixtures.join(format!("applicability-{name}")),
            &crates.join(format!("applicability-{name}")),
        );
    }
    let project = |name: &str, source: &str| {
        let root = base.join(name);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[workspace]\n[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-a={{path={:?}}}\nfacts-c={{path={:?}}}\n",
                crates.join("applicability-a"),
                crates.join("applicability-c"),
            ),
        )
        .unwrap();
        fs::copy(
            Path::new(FIXTURE).join("rust-toolchain.toml"),
            root.join("rust-toolchain.toml"),
        )
        .unwrap();
        fs::write(root.join("src/main.rs"), source).unwrap();
        root
    };
    let run = |root: &Path, expected: &str| {
        let run = rawr_import_within(root, &["run"], &home, std::time::Duration::from_secs(600));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), expected);
    };
    // Each published entry: its key directory and its publication.
    let published = |root: &Path| {
        let mut entries = fs::read_dir(root.join("target/rawr/inventories"))
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        let path = entry.ok()?.path();
                        let generation = fs::read_to_string(path.join(".generation")).ok()?;
                        let entry = fs::read_to_string(path.join("entry.json")).ok()?;
                        let entry = serde_json::from_str::<serde_json::Value>(&entry).ok()?;
                        let name = ["facts_a", "facts_b", "facts_c"]
                            .into_iter()
                            .find(|name| entry["crate_name"] == *name)?;
                        Some((name, path, generation))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        entries.sort();
        entries
    };

    let first = project(
        "first",
        "// rawr\nfn left() -> i64 {\n    use facts_c::Left;\n    facts_a::both().choose()\n}\nfn right() -> bool {\n    use facts_c::Right;\n    facts_a::both().choose()\n}\nfn bumped() -> i64 {\n    use facts_c::BumpLeft;\n    let mut both = facts_a::both();\n    both.bump()\n}\nfn taken() -> i64 {\n    use facts_c::TakeLeft;\n    facts_a::both().spend()\n}\nfn main() {\n    println!(\"{} {} {} {}\", left(), right(), bumped(), taken());\n}\n",
    );
    run(&first, "11 true 41 51\n");
    let shared = published(&first);
    assert_eq!(
        shared.iter().map(|(name, ..)| *name).collect::<Vec<_>>(),
        ["facts_a", "facts_b", "facts_c"],
        "each crate of the closure has its own entry"
    );
    // A later run of the project reads every entry back.
    run(&first, "11 true 41 51\n");
    assert_eq!(published(&first), shared, "a later run reuses every entry");
    let helpers =
        fs::read_to_string(first.join("target/rawr/runner/src/rawr_applications.rs")).unwrap();
    for expected in [
        "#[no_implicit_prelude]",
        "use ::facts_c::Left as _;",
        "use ::facts_c::Right as _;",
        "pub(crate) fn call(receiver: &::facts_a::Both) -> i64 {",
        "pub(crate) fn call(receiver: &::facts_a::Both) -> bool {",
        "(*receiver).choose()",
        "pub(crate) fn call(receiver: &mut ::facts_a::Both) -> i64 {",
        "(*receiver).bump()",
        "pub(crate) fn call(receiver: ::facts_a::Both) -> i64 {",
        "receiver.spend()",
    ] {
        assert!(helpers.contains(expected), "{expected}\n{helpers}");
    }
    assert!(!helpers.contains("use super::*"));
    // A second project, its blocks' imports swapped: each call re-selects.
    let second = project(
        "second",
        "// rawr\nfn first() {\n    use facts_c::Right;\n    println!(\"{}\", facts_a::both().choose());\n}\nfn second() {\n    use facts_c::Left;\n    println!(\"{}\", facts_a::both().choose());\n}\nfn main() {\n    first();\n    second();\n}\n",
    );
    run(&second, "true\n11\n");

    // A changed crate has a new key; the others keep theirs.
    let source = crates.join("applicability-c/src/lib.rs");
    let mut text_c = fs::read_to_string(&source).unwrap();
    text_c.push_str("\npub const CHANGED: i64 = 1;\n");
    fs::write(&source, text_c).unwrap();
    run(&first, "11 true 41 51\n");
    let changed = published(&first);
    let kept = |name: &str| {
        changed
            .iter()
            .filter(|(entry, ..)| *entry == name)
            .cloned()
            .collect::<Vec<_>>()
    };
    for name in ["facts_a", "facts_b"] {
        assert_eq!(
            kept(name),
            shared
                .iter()
                .filter(|(entry, ..)| *entry == name)
                .cloned()
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        kept("facts_c").len(),
        2,
        "the changed crate has a second entry"
    );
}

/// A save that calls a new method on a type the program already binds, from
/// a trait already in scope, runs no nightly process (D609.6): the method is
/// proved from the crates' intrinsic facts, read back by the API's inputs.
#[test]
fn a_new_member_of_an_unchanged_graph_is_proved_with_no_nightly_run() {
    let base = crate::support::scratch().join("new-member-no-nightly");
    let home = base.join("home");
    let crates = base.join("crates");
    let root = base.join("app");
    let _ = fs::remove_dir_all(&base);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../importer/tests/fixtures");
    for name in ["a", "b", "c"] {
        copy(
            &fixtures.join(format!("applicability-{name}")),
            &crates.join(format!("applicability-{name}")),
        );
    }
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\n[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-a={{path={:?}}}\nfacts-c={{path={:?}}}\n",
            crates.join("applicability-a"),
            crates.join("applicability-c"),
        ),
    )
    .unwrap();
    fs::copy(
        Path::new(FIXTURE).join("rust-toolchain.toml"),
        root.join("rust-toolchain.toml"),
    )
    .unwrap();
    let run = |expected: &str| {
        let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(600));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), expected);
    };
    // Every nightly process leaves its trace: the nightly check build marks
    // each crate it compiles, and each extraction writes its plan.
    let nightly = |since: std::time::SystemTime| {
        fn walk(path: &Path, since: std::time::SystemTime, found: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(path) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, since, found);
                } else if path
                    .file_name()
                    .is_some_and(|name| !name.to_string_lossy().ends_with(".lock"))
                    && entry
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .is_ok_and(|modified| modified > since)
                {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        for directory in ["extraction", "cache/resolution", "inventories"] {
            walk(&root.join("target/rawr").join(directory), since, &mut found);
        }
        walk(&nightly_directory(&home), since, &mut found);
        found
    };
    let started = std::time::SystemTime::now();
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse facts_c::Extension;\nfn left() -> i64 {\n    use facts_c::Left;\n    facts_a::both().choose()\n}\nfn main() {\n    let _bound = facts_a::left();\n    println!(\"{}\", left());\n}\n",
    )
    .unwrap();
    run("11\n");
    assert!(
        !nightly(started).is_empty(),
        "the first run acquires the API and the facts on the nightly"
    );
    // The first run's check was keyed by its facts as soon as it needed
    // them, so the next run compiles nothing: the store gains no node (#400).
    let stored = || {
        fn walk(path: &Path, found: &mut std::collections::BTreeSet<PathBuf>) {
            for entry in fs::read_dir(path).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else if !path.to_string_lossy().ends_with(".lock") {
                    found.insert(path);
                }
            }
        }
        let mut found = std::collections::BTreeSet::new();
        walk(&root.join("target/rawr/store"), &mut found);
        found
    };
    let before = stored();
    run("11\n");
    let added = stored().difference(&before).cloned().collect::<Vec<_>>();
    assert!(added.is_empty(), "the second run compiled again: {added:?}");
    run("11\n");

    std::thread::sleep(std::time::Duration::from_millis(1100));
    let saved = std::time::SystemTime::now();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse facts_c::Extension;\nfn left() -> i64 {\n    use facts_c::Left;\n    facts_a::both().choose()\n}\nfn extended() -> i64 {\n    facts_a::left().extension()\n}\nfn main() {\n    let _bound = facts_a::left();\n    println!(\"{} {}\", left(), extended());\n}\n",
    )
    .unwrap();
    run("11 101\n");
    let traces = nightly(saved);
    assert_eq!(traces.len(), 0, "nightly processes ran: {traces:?}");
}

/// A crate whose whole closure comes from a registry keeps its intrinsic
/// entry in rawr's home under a machine-wide key: a second project reads it
/// back instead of publishing its own. A closure that runs a build script
/// gets there once the project's build gave its scripts the outputs the
/// nightly's did (#403). Path crates keep theirs per project.
#[test]
fn an_immutable_crates_intrinsic_entry_is_shared_per_machine() {
    let base = crate::support::scratch().join("shared-inventories");
    let home = base.join("home");
    let crates = base.join("crates");
    let _ = fs::remove_dir_all(&base);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../importer/tests/fixtures");
    for name in ["a", "b", "c"] {
        copy(
            &fixtures.join(format!("applicability-{name}")),
            &crates.join(format!("applicability-{name}")),
        );
    }
    let project = |name: &str| {
        let root = base.join(name);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[workspace]\n[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-a={{path={:?}}}\nfacts-c={{path={:?}}}\nsemver='1'\ncamino='1'\n",
                crates.join("applicability-a"),
                crates.join("applicability-c"),
            ),
        )
        .unwrap();
        fs::copy(
            Path::new(FIXTURE).join("rust-toolchain.toml"),
            root.join("rust-toolchain.toml"),
        )
        .unwrap();
        fs::write(
            root.join("src/main.rs"),
            "// rawr\nfn left() -> i64 {\n    use facts_c::Left;\n    facts_a::both().choose()\n}\nfn main() {\n    println!(\"{} {}\", left(), semver::Version::new(1, 2, 3));\n}\n",
        )
        .unwrap();
        let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(600));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), "11 1.2.3\n");
        root
    };
    // Each entry in a store: its crate and its publication.
    let entries = |store: &Path| {
        let mut entries = fs::read_dir(store)
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        let path = entry.ok()?.path();
                        let generation = fs::read_to_string(path.join(".generation")).ok()?;
                        let entry = fs::read_to_string(path.join("entry.json")).ok()?;
                        let entry = serde_json::from_str::<serde_json::Value>(&entry).ok()?;
                        Some((entry["crate_name"].as_str()?.to_owned(), generation))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        entries.sort();
        entries
    };
    let first = project("first");
    let shared = entries(&home.join("inventories"));
    assert_eq!(
        shared
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["camino", "semver"],
        "only the registry crates' entries are machine-wide"
    );
    // camino runs a build script: the first project captured its entry, and
    // published it once its own build gave the nightly's outputs (#403).
    let local = entries(&first.join("target/rawr/inventories"));
    let local = local
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    for name in ["facts_a", "facts_b", "facts_c", "camino"] {
        assert!(local.contains(&name), "{name} is kept locally: {local:?}");
    }
    let second = project("second");
    assert_eq!(
        entries(&home.join("inventories")),
        shared,
        "the second project reads the shared entries back"
    );
    assert!(
        !entries(&second.join("target/rawr/inventories"))
            .iter()
            .any(|(name, _)| name == "camino" || name == "semver"),
        "the second project captures neither"
    );
}

/// A crate's bindings are shared per machine: a second project with the
/// same program reads them from rawr's home, with no nightly build and no
/// probe, and its next run takes the resolution back with no Cargo. semver
/// is small enough to be read whole, so a program that uses other items of
/// it reads them from the home too (D609.4).
#[test]
fn a_second_project_reads_the_first_ones_bindings_with_no_nightly_build() {
    // This test owns its binding namespace. Old protocol generations in the
    // suite's general home are valid retained entries, not extra publications
    // by this run. Preserve the runtime while starting with cold bindings.
    let home = crate::support::scratch().join("semver-sharing-home");
    let bindings = home.join("bindings");
    let _ = fs::remove_dir_all(&bindings);
    let projects = ["first", "second", "other", "other-again"]
        .map(|name| crate::support::scratch().join(format!("shared-{name}")));
    for root in &projects {
        let _ = fs::remove_dir_all(root);
        copy(Path::new(FIXTURE), root);
    }
    let [first, second, other, again] = &projects;
    let requirement = "// rawr\nuse semver::{Version, VersionReq};\n\nfn main() {\n    let requirement = VersionReq::parse(\">=1.2.0, <2\").unwrap();\n    println!(\"{}\", requirement.matches(&Version::parse(\"1.4.0\").unwrap()));\n}\n";
    for root in [other, again] {
        fs::write(root.join("src/main.rs"), requirement).unwrap();
    }
    let expected = "parsed: 1.2.3\nmajor: 1\nwidest: 18446744073709551615.0.9223372036854775808\n";
    let run = |root: &Path, stdout: &str| {
        let run = rawr_import_with_home(root, &["run"], &home);
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), stdout);
    };
    let nightly = |root: &Path| {
        let probed = fs::read_dir(root.join("target/rawr/extraction"))
            .map(|entries| entries.count())
            .unwrap_or_default();
        nightly_built(root) || probed != 0
    };

    run(first, expected);
    assert!(
        nightly(first),
        "the first project starts with cold bindings"
    );
    let published = fs::read_dir(&bindings)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let meta = fs::read_to_string(path.join("meta.json")).ok()?;
            meta.contains("\"crate_name\":\"semver\"").then_some(path)
        })
        .collect::<Vec<_>>();
    assert_eq!(published.len(), 1, "{published:?}");
    let table = fs::read_to_string(published[0].join("table.json")).unwrap();
    assert!(table.contains("\"whole\":true"), "semver is read whole");

    // The same program in a second project: no nightly build, and a run
    // after it takes its resolution back.
    run(second, expected);
    assert!(!nightly(second));
    run(second, expected);
    assert!(second.join("target/rawr/shared/remembered.json").is_file());
    // With the project's store gone, the remembered run builds its API
    // inside the store's batch, under the project's ownership lease. It
    // reads what the API is built from off disk: running the resolution
    // there again waited on that lease forever.
    let harness = second.join("target/rawr/shared/Cargo.toml");
    let copied = fs::metadata(&harness).unwrap().modified().unwrap();
    fs::remove_dir_all(second.join("target/rawr/store")).unwrap();
    let rebuilt = rawr_import_within(second, &["run"], &home, std::time::Duration::from_secs(300));
    assert!(
        rebuilt.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&rebuilt.stdout),
        text(&rebuilt.stderr)
    );
    assert_eq!(text(&rebuilt.stdout), expected);
    assert_eq!(
        fs::metadata(&harness).unwrap().modified().unwrap(),
        copied,
        "the remembered run resolved again"
    );

    // Other items of the crate replay from the whole table, and the same
    // program in another project takes them from the home again.
    run(other, "true\n");
    assert!(!nightly(other));
    run(again, "true\n");
    assert!(!nightly(again));
}

/// `rawr check` publishes what it extracts once the `cargo check` beside it
/// gives the nightly's build-script outputs, as a run does after its runner
/// build: a second project's check then reads the bindings from rawr's home,
/// with no nightly build and no probe (D609.4, #661).
#[test]
fn a_second_project_checks_on_the_first_checks_bindings_with_no_nightly_build() {
    let home = crate::support::scratch().join("checked-sharing-home");
    let _ = fs::remove_dir_all(home.join("bindings"));
    let projects =
        ["first", "second"].map(|name| crate::support::scratch().join(format!("checked-{name}")));
    for root in &projects {
        let _ = fs::remove_dir_all(root);
        copy(Path::new(FIXTURE), root);
    }
    let [first, second] = &projects;
    let check = |root: &Path| {
        let checked = rawr_import_with_home(root, &["check"], &home);
        assert!(
            checked.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&checked.stdout),
            text(&checked.stderr)
        );
        assert!(text(&checked.stdout).contains("cargo check passed"));
    };
    let nightly = |root: &Path| {
        let probed = fs::read_dir(root.join("target/rawr/extraction"))
            .map(|entries| entries.count())
            .unwrap_or_default();
        nightly_built(root) || probed != 0
    };

    check(first);
    assert!(
        nightly(first),
        "the first project starts with cold bindings"
    );
    assert!(
        !first.join("target/rawr/runner").exists(),
        "a check builds no runner"
    );
    let published = fs::read_dir(home.join("bindings"))
        .unwrap()
        .filter(|entry| {
            let meta = entry.as_ref().unwrap().path().join("meta.json");
            fs::read_to_string(meta).is_ok_and(|meta| meta.contains("\"crate_name\":\"semver\""))
        })
        .count();
    assert_eq!(published, 1, "the check published semver's bindings");

    check(second);
    assert!(!nightly(second), "the second project reads the home");
}

/// A program with a module of its own selects its own crate, whose table
/// stays with its project. A second project extracts only that crate: the
/// dependency's table, trait members included, replays from rawr's home
/// as it is, though the program's own crate is selected beside it (#672).
#[test]
fn a_second_project_whose_program_has_a_module_replays_its_dependencies_from_the_home() {
    let home = crate::support::scratch().join("module-sharing-home");
    let bindings = home.join("bindings");
    let _ = fs::remove_dir_all(&bindings);
    let projects =
        ["first", "second"].map(|name| crate::support::scratch().join(format!("module-{name}")));
    for root in &projects {
        let _ = fs::remove_dir_all(root);
        copy(Path::new(FIXTURE), root);
        fs::write(
            root.join("src/main.rs"),
            "// rawr\nmod label;\n\nuse semver::{Version, VersionReq};\n\nfn main() {\n    let version = Version::parse(\"1.2.3\").unwrap();\n    println!(\"{}\", label::describe(\"1.2.3\"));\n    println!(\"{}\", VersionReq::default().matches(&version));\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("src/label.rs"),
            "use semver::Version;\n\npub fn describe(text: &str) -> String {\n    let version = Version::parse(text).unwrap();\n    format!(\"{}.{}\", version.major, version.minor)\n}\n",
        )
        .unwrap();
    }
    let [first, second] = &projects;
    let check = |root: &Path| {
        let checked = rawr_import_with_home(root, &["check"], &home);
        assert!(
            checked.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&checked.stdout),
            text(&checked.stderr)
        );
        assert!(text(&checked.stdout).contains("cargo check passed"));
    };
    // The shared table of the crate named `crate_name`, and its key.
    let shared = |crate_name: &str| {
        fs::read_dir(&bindings)
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let meta = fs::read_to_string(path.join("meta.json")).ok()?;
                let meta = serde_json::from_str::<serde_json::Value>(&meta).ok()?;
                (meta["crate_name"] == crate_name).then(|| {
                    (
                        path.file_name().unwrap().to_owned(),
                        fs::read(path.join("table.json")).unwrap(),
                    )
                })
            })
            .collect::<Vec<_>>()
    };

    check(first);
    let published = shared("semver");
    assert_eq!(published.len(), 1, "the check published semver's bindings");
    let (key, table) = &published[0];
    let table = String::from_utf8(table.clone()).unwrap();
    assert!(
        table.contains("\"Member\""),
        "the program names a trait member of semver's: {table}"
    );
    assert!(
        shared("plain_dependency").is_empty(),
        "the program's own crate stays with its project"
    );

    check(second);
    // The second project probed its own crate, and replayed semver's table
    // as the home had it. A table the probe extracted, even in part, is
    // recorded again beside it with what it served.
    let tables = second.join("target/rawr/extraction");
    let replayed = fs::read(tables.join(key)).unwrap();
    assert_eq!(
        String::from_utf8(replayed).unwrap(),
        table,
        "semver's table replays unchanged"
    );
    let mut served = key.clone();
    served.push(".served");
    assert!(
        !tables.join(served).exists(),
        "the probe extracted none of semver"
    );
    assert_eq!(shared("semver").len(), 1);
}

/// A shared crate's table holds only what its key covers: the crates its
/// own closure reaches. `feet` implements `From<Feet>` for `meters`'s
/// `Meters`, which `meters::total(impl Into<Meters>)` names, but `meters`
/// does not reach `feet`. Each project builds `feet` with other features,
/// so another `feet`, and both share `meters`'s key. Whichever project
/// checks second replays `meters`'s table from the home, and it must not
/// name the other project's `feet` (#1097).
#[test]
fn a_shared_table_names_no_crate_outside_its_closure() {
    let root = crate::support::scratch().join("shared-closure");
    let _ = fs::remove_dir_all(&root);
    let repository = root.join("repository");
    for (name, manifest, source) in [
        (
            "meters",
            "[package]\nname='meters'\nversion='0.1.0'\nedition='2021'\n",
            "pub struct Meters(pub f32);\n\npub fn total(length: impl Into<Meters>) -> f32 {\n    length.into().0\n}\n",
        ),
        (
            "feet",
            "[package]\nname='feet'\nversion='0.1.0'\nedition='2021'\n[features]\nloud=[]\n[dependencies]\nmeters={path='../meters'}\n",
            "pub struct Feet(pub f32);\n\nimpl From<Feet> for meters::Meters {\n    fn from(feet: Feet) -> Self {\n        meters::Meters(feet.0 * 0.3048)\n    }\n}\n",
        ),
    ] {
        let package = repository.join(name);
        fs::create_dir_all(package.join("src")).unwrap();
        fs::write(package.join("Cargo.toml"), manifest).unwrap();
        fs::write(package.join("src/lib.rs"), source).unwrap();
    }
    fs::write(
        repository.join("Cargo.toml"),
        "[workspace]\nmembers=['meters','feet']\nresolver='2'\n",
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "shared closure fixture",
        ],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&repository)
            .status()
            .unwrap()
            .success());
    }
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repository)
        .output()
        .unwrap();
    assert!(revision.status.success());
    let revision = text(&revision.stdout);
    let source = format!(
        "git='file://{}',rev='{}'",
        repository.display(),
        revision.trim()
    );
    let project = |name: &str, features: &str| {
        let project = root.join(name);
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname='shared-closure-{name}'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nmeters={{{source}}}\nfeet={{{source},features=[{features}]}}\n[workspace]\n"),
        )
        .unwrap();
        fs::write(
            project.join("src/main.rs"),
            "// rawr\nuse feet::Feet;\nuse meters::{total, Meters};\n\nfn main() {\n    let feet = Feet(10.0);\n    println!(\"{} {}\", total(Meters(2.0)), feet.0);\n}\n",
        )
        .unwrap();
        project
    };
    let loud = project("loud", "'loud'");
    let quiet = project("quiet", "");
    // The `meters` tables in the home's bindings.
    let shared = |home: &Path| {
        fs::read_dir(home.join("bindings"))
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let meta = fs::read_to_string(path.join("meta.json")).ok()?;
                let meta = serde_json::from_str::<serde_json::Value>(&meta).ok()?;
                (meta["crate_name"] == "meters")
                    .then(|| fs::read_to_string(path.join("table.json")).unwrap())
            })
            .collect::<Vec<_>>()
    };
    // Either project may check first.
    for (order, [first, second]) in [
        ("loud-first", [&loud, &quiet]),
        ("quiet-first", [&quiet, &loud]),
    ] {
        let home = root.join(format!("home-{order}"));
        for project in [first, second] {
            let _ = fs::remove_dir_all(project.join("target"));
            let checked = rawr_import_with_home(project, &["check"], &home);
            assert!(
                checked.status.success(),
                "{order}, {}:\nstdout:\n{}\nstderr:\n{}",
                project.display(),
                text(&checked.stdout),
                text(&checked.stderr)
            );
        }
        let tables = shared(&home);
        assert_eq!(
            tables.len(),
            1,
            "{order}: both projects share meters's table"
        );
        assert!(
            !tables[0].contains("\"feet\""),
            "{order}: meters's table names feet, which meters does not reach"
        );
    }
}

/// The declared inputs of a shared immutable crate are checked before both
/// `check` and a remembered warm run consume its API. The unchanged path still
/// shares across projects without a nightly build.
#[test]
fn shared_git_bindings_validate_declared_file_and_environment_before_replay() {
    let root = crate::support::scratch().join("shared-script-inputs");
    let _ = fs::remove_dir_all(&root);
    let dependency = root.join("dependency");
    fs::create_dir_all(dependency.join("src")).unwrap();
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname='binding-input-fixture'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        dependency.join("src/lib.rs"),
        "#[cfg(narrow)]\npub fn only_when_narrow() -> usize { width() }\npub fn width() -> usize { env!(\"BINDING_WIDTH\").len() }\n",
    )
    .unwrap();
    fs::write(
        dependency.join("build.rs"),
        r#"
fn main() {
    let path = std::env::var("BINDING_INPUT_FILE").unwrap();
    let mode = std::env::var("BINDING_INPUT_MODE").unwrap();
    let input = std::fs::read_to_string(&path).unwrap();
    println!("cargo:rerun-if-env-changed=BINDING_INPUT_FILE");
    println!("cargo:rerun-if-env-changed=BINDING_INPUT_MODE");
    println!("cargo:rerun-if-changed={path}");
    println!("cargo:rustc-check-cfg=cfg(narrow)");
    println!("cargo:rustc-env=BINDING_WIDTH={input}{mode}");
    if input == "short" && mode == "small" {
        println!("cargo:rustc-cfg=narrow");
    }
}
"#,
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "binding input fixture",
        ],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&dependency)
            .status()
            .unwrap()
            .success());
    }
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dependency)
        .output()
        .unwrap();
    assert!(revision.status.success());
    let revision = text(&revision.stdout);
    let input = root.join("declared.txt");
    fs::write(&input, "short").unwrap();
    let projects = ["first", "checked", "warm"].map(|name| root.join(name));
    for project in &projects {
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("Cargo.toml"), format!("[package]\nname='binding-input-app'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nbinding-input-fixture={{git='file://{}',rev='{}'}}\n[workspace]\n", dependency.display(), revision.trim())).unwrap();
        fs::write(project.join("src/main.rs"), "// rawr\nuse binding_input_fixture::only_when_narrow;\nfn main() { println!(\"{}\", only_when_narrow()); }\n").unwrap();
    }
    let run = |project: &Path, action: &str, mode: &str| {
        Command::new(env!("CARGO_BIN_EXE_rawr-import"))
            .args([action])
            .arg(project)
            .env("RAWR_HOME", crate::support::runtime_home())
            .env("BINDING_INPUT_FILE", &input)
            .env("BINDING_INPUT_MODE", mode)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap()
    };
    for project in &projects {
        let output = run(project, "run", "small");
        assert!(
            output.status.success(),
            "{}\n{}",
            text(&output.stdout),
            text(&output.stderr)
        );
        assert_eq!(text(&output.stdout), "10\n");
    }
    for project in &projects[1..] {
        assert!(
            !nightly_built(project),
            "unchanged crate should replay without nightly: {}",
            project.display()
        );
        let output = run(project, "run", "small");
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert!(!nightly_built(project));
        assert!(project.join("target/rawr/shared/remembered.json").is_file());
    }
    let checked = run(&projects[1], "check", "small");
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(!nightly_built(&projects[1]));
    fs::write(&input, "long").unwrap();
    let changed = run(&projects[1], "check", "small");
    assert!(
        !changed.status.success(),
        "changed file replayed old API: {}",
        text(&changed.stdout)
    );
    assert!(
        !text(&changed.stdout).contains("Checked "),
        "Rawr accepted the stale API before Cargo diagnosed it"
    );
    assert!(
        text(&changed.stderr).contains("only_when_narrow"),
        "{}",
        text(&changed.stderr)
    );
    fs::write(&input, "short").unwrap();
    let changed = run(&projects[2], "run", "large");
    assert!(
        !changed.status.success(),
        "changed environment replayed old API: {}",
        text(&changed.stdout)
    );
    assert!(
        text(&changed.stderr).contains("only_when_narrow"),
        "{}",
        text(&changed.stderr)
    );

    // A fresh shared hit, then a remembered hit, must retain the external
    // declared path in the running program's watches. Its source stays put.
    let live = root.join("live");
    fs::create_dir_all(live.join("src")).unwrap();
    fs::copy(projects[0].join("Cargo.toml"), live.join("Cargo.toml")).unwrap();
    fs::write(live.join("src/main.rs"), "// rawr\nuse binding_input_fixture::width;\nfn main() { println!(\"width {}\", width()); let stdin = std::io::stdin(); let mut line = String::new(); stdin.read_line(&mut line).unwrap(); }\n").unwrap();
    let start = || {
        crate::support::start_with(
            &live,
            &[
                ("BINDING_INPUT_FILE", input.clone()),
                ("BINDING_INPUT_MODE", PathBuf::from("small")),
            ],
            &[],
            std::process::Stdio::piped(),
        )
    };
    let finish = |run: &mut crate::support::Run| {
        use std::io::Write as _;
        run.child.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(
            run.child.wait().unwrap().success(),
            "{}",
            run.seen.join("\n")
        );
    };
    let mut shared = start();
    shared.until(std::time::Duration::from_secs(300), |line| {
        line == "width 10"
    });
    assert!(!nightly_built(&live));
    finish(&mut shared);
    assert!(live.join("target/rawr/shared/remembered.json").is_file());
    let mut remembered = start();
    remembered.until(std::time::Duration::from_secs(120), |line| {
        line == "width 10"
    });
    assert!(!nightly_built(&live));
    fs::write(&input, "longer").unwrap();
    remembered.until(std::time::Duration::from_secs(180), |line| {
        line == "width 11"
    });
    assert!(
        remembered
            .seen
            .iter()
            .any(|line| line.contains("restarting the program")),
        "{}",
        remembered.seen.join("\n")
    );
    finish(&mut remembered);
}

/// A real adapter build reports the selected crate when its nightly API
/// differs from the API that the project's stable compiler provides.
#[test]
fn an_adapter_build_names_a_crate_whose_api_differs_between_toolchains() {
    let root = crate::support::scratch().join("shared-adapter-drift");
    let _ = fs::remove_dir_all(&root);
    let dependency = root.join("dependency");
    fs::create_dir_all(dependency.join("src")).unwrap();
    fs::write(
        dependency.join("Cargo.toml"),
        "[workspace]\n[package]\nname='adapter-drift-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        dependency.join("src/lib.rs"),
        "#[cfg(nightly_api)]\npub fn value() -> u32 { 7 }\n#[cfg(not(nightly_api))]\npub fn value() -> &'static str { \"seven\" }\n",
    )
    .unwrap();
    fs::write(
        dependency.join("build.rs"),
        r#"fn main() {
    let rustc = std::env::var_os("RUSTC").unwrap();
    let version = std::process::Command::new(rustc).arg("--version").output().unwrap();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(nightly_api)");
    if String::from_utf8(version.stdout).unwrap().contains("nightly") {
        println!("cargo:rustc-cfg=nightly_api");
    }
}
"#,
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "adapter drift fixture",
        ],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&dependency)
            .status()
            .unwrap()
            .success());
    }
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dependency)
        .output()
        .unwrap();
    assert!(revision.status.success());
    let project = root.join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        format!("[workspace]\n[package]\nname='adapter-drift-app'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nadapter-drift-fixture={{git='file://{}',rev='{}'}}\n", dependency.display(), text(&revision.stdout).trim()),
    )
    .unwrap();
    fs::write(
        project.join("src/main.rs"),
        "// rawr\nuse adapter_drift_fixture::value;\nfn main() { let result: u32 = value(); println!(\"{result}\"); }\n",
    )
    .unwrap();
    let home = crate::support::scratch().join("rawr-home");
    let output = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&project)
        .env("RAWR_HOME", &home)
        .env("RUSTUP_TOOLCHAIN", "stable")
        .env_remove("RUSTC")
        .env_remove("CARGO_BUILD_RUSTC")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let diagnostic = text(&output.stderr);
    assert!(!output.status.success(), "{}", text(&output.stdout));
    assert!(
        diagnostic.contains("the API of `adapter_drift_fixture` differs between the extraction nightly and the project's toolchain"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("mismatched types"), "{diagnostic}");
    // The first failure may be a stale run in the shared nightly directory:
    // it runs again once, and the same difference after that is drift
    // (D742), reported once.
    assert_eq!(
        diagnostic
            .matches("differs from this project's build: running it again")
            .count(),
        1,
        "{diagnostic}"
    );
    assert_eq!(
        diagnostic.matches("after running it again too").count(),
        1,
        "{diagnostic}"
    );
    let published = fs::read_dir(home.join("bindings"))
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path().join("meta.json");
            let meta: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
            (meta["crate_name"] == "adapter_drift_fixture").then_some(meta)
        })
        .count();
    assert_eq!(published, 0, "a divergent API was shared");
}

/// A dependency's build script that reads an environment variable it never
/// declares keeps, in the shared nightly directory, the output of whichever
/// project last ran it (Cargo's rule). A project whose own build gives the
/// script other output runs it again there once, and its program prints its
/// own value, as `cargo run` does (D742). `check` does the same.
#[test]
fn an_undeclared_build_script_input_runs_again_for_the_project_whose_build_differs() {
    let root = crate::support::scratch().join("shared-undeclared-env");
    let _ = fs::remove_dir_all(&root);
    let dependency = root.join("dependency");
    fs::create_dir_all(dependency.join("src")).unwrap();
    fs::write(
        dependency.join("Cargo.toml"),
        "[workspace]\n[package]\nname='undeclared-env-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        dependency.join("src/lib.rs"),
        "const fn parse(text: &str) -> u32 {\n    let bytes = text.as_bytes();\n    let mut value = 0;\n    let mut index = 0;\n    while index < bytes.len() {\n        value = value * 10 + (bytes[index] - b'0') as u32;\n        index += 1;\n    }\n    value\n}\npub const VALUE: u32 = parse(env!(\"UNDECLARED_VALUE\"));\n",
    )
    .unwrap();
    // Declares only itself: the variable it reads is an undeclared input.
    fs::write(
        dependency.join("build.rs"),
        r#"fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let value = std::env::var("RAWR_UNDECLARED_VALUE").unwrap();
    println!("cargo:rustc-env=UNDECLARED_VALUE={value}");
}
"#,
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "undeclared environment fixture",
        ],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&dependency)
            .status()
            .unwrap()
            .success());
    }
    let revision = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dependency)
        .output()
        .unwrap();
    assert!(revision.status.success());
    let revision = text(&revision.stdout);
    let projects = ["first", "second", "checked"].map(|name| root.join(name));
    for project in &projects {
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            format!("[workspace]\n[package]\nname='undeclared-env-app'\nversion='0.1.0'\nedition='2021'\n[dependencies]\nundeclared-env-fixture={{git='file://{}',rev='{}'}}\n", dependency.display(), revision.trim()),
        )
        .unwrap();
        fs::write(
            project.join("src/main.rs"),
            "// rawr\nfn main() {\n    println!(\"value {}\", undeclared_env_fixture::VALUE);\n}\n",
        )
        .unwrap();
    }
    let home = root.join("rawr-home");
    let run = |project: &Path, action: &str, value: &str| {
        Command::new(env!("CARGO_BIN_EXE_rawr-import"))
            .arg(action)
            .arg(project)
            .env("RAWR_HOME", &home)
            .env("RAWR_RUNTIME_HOME", crate::support::runtime_home())
            .env("RAWR_UNDECLARED_VALUE", value)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap()
    };
    let reruns = |output: &Output| {
        text(&output.stderr)
            .matches("differs from this project's build: running it again")
            .count()
    };
    let first = run(&projects[0], "run", "1");
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert_eq!(text(&first.stdout), "value 1\n");
    assert_eq!(reruns(&first), 0, "{}", text(&first.stderr));
    // The nightly's run of the script holds the first project's value; the
    // second project's build gives its own.
    let second = run(&projects[1], "run", "2");
    assert!(second.status.success(), "{}", text(&second.stderr));
    assert_eq!(
        text(&second.stdout),
        "value 2\n",
        "{}",
        text(&second.stderr)
    );
    assert_eq!(reruns(&second), 1, "{}", text(&second.stderr));
    assert!(
        !text(&second.stderr).contains("toolchain"),
        "a stale shared run was blamed on the toolchain: {}",
        text(&second.stderr)
    );
    // What it extracted is shared under its value: its next run replays it.
    let again = run(&projects[1], "run", "2");
    assert!(again.status.success(), "{}", text(&again.stderr));
    assert_eq!(text(&again.stdout), "value 2\n");
    assert_eq!(reruns(&again), 0, "{}", text(&again.stderr));
    let checked = run(&projects[2], "check", "3");
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert_eq!(reruns(&checked), 1, "{}", text(&checked.stderr));
    let ran = run(&projects[2], "run", "3");
    assert!(ran.status.success(), "{}", text(&ran.stderr));
    assert_eq!(text(&ran.stdout), "value 3\n", "{}", text(&ran.stderr));
    assert_eq!(reruns(&ran), 0, "{}", text(&ran.stderr));
    let _ = fs::remove_dir_all(&root);
}

/// A marked `main` gets the arguments after `--` and stdin, as under
/// `cargo run`, and a save while it waits on stdin restarts it on the same
/// stdin (D500.1).
#[test]
fn a_marked_main_gets_its_arguments_and_stdin_and_restarts_on_a_save() {
    use std::io::Write as _;
    use std::time::Duration;

    const ARGUMENTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plain-arguments"
    );
    let root = crate::support::scratch().join("plain-arguments");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(ARGUMENTS), &root);
    let main = root.join("src/main.rs");
    let mut run = crate::support::start_with(
        &root,
        &[],
        &["one", "two words"],
        std::process::Stdio::piped(),
    );
    let mut stdin = run.child.stdin.take().unwrap();

    let name = run.until(Duration::from_secs(900), |line| line.starts_with("name "));
    assert!(
        name.ends_with("/target/debug/plain-arguments"),
        "{}",
        run.seen.join("\n")
    );
    run.until(Duration::from_secs(30), |line| line == "arg one");
    run.until(Duration::from_secs(30), |line| line == "arg two words");
    stdin.write_all(b"1.2.3\n").unwrap();
    run.until(Duration::from_secs(30), |line| line == "major 1");

    // The program waits on stdin; a save restarts it with the same
    // arguments, and the next line goes to the new one.
    std::thread::sleep(Duration::from_secs(2));
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.replace("\"major\"", "\"Major\"")).unwrap();
    run.until(Duration::from_secs(120), |line| line.starts_with("name "));
    assert!(
        run.seen
            .iter()
            .any(|line| line.ends_with("rawr: the source changed; restarting the program")),
        "{}",
        run.seen.join("\n")
    );
    run.until(Duration::from_secs(30), |line| line == "arg two words");
    stdin.write_all(b"4.5.6\n").unwrap();
    run.until(Duration::from_secs(30), |line| line == "Major 4");
    drop(stdin);
    run.until(Duration::from_secs(30), |line| line == "done");
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A dependency's `macro_rules!` macros expand from its source as rustc
/// expands them, and the program prints what `cargo run` prints: `$crate`
/// paths into a crate the program does not name and to `#[doc(hidden)]`
/// items, repetitions with separators, a macro calling itself through
/// `$crate`, `stringify!` of a fragment, a trait method called by its path,
/// and a local of the macro's own beside the caller's (T215).
#[test]
fn a_dependencys_macro_rules_expand_from_its_source() {
    const MACROS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-macros");
    let root = crate::support::scratch().join("plain-macros");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(MACROS), &root);

    let run = rawr_import(&root, &["run"]);

    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        "0 3 22\n8\n40 8\n116\n(\"0x123\", 5)\n220 220\n"
    );
}

/// A crate's derive on a program type is expanded by rustc and its items
/// are program code (D740, #593). A local proc-macro crate's derive,
/// re-exported beside its trait as serde's is, runs on a top-level struct
/// and enum, a struct in a function's body and one in an inline module
/// named by its path, and prints what `cargo run` prints. So do serde's
/// `Serialize` and `Deserialize` through serde_json, on the saved state of
/// iced's todos too.
#[test]
fn a_crates_derive_runs_from_its_expansion() {
    const DERIVES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-derives");
    let root = crate::support::scratch().join("plain-derives");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(DERIVES), &root);

    // serde's `Serialize` and `Deserialize`, from their expansions, run
    // as `cargo run` runs them.
    let run = rawr_import(&root, &["run"]);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        "{\"description\":\"write\",\"done\":false}\nTask { description: \"write\", done: false }\n"
    );

    // iced's todos' saved state: a field `#[serde(default = "..")]` fills
    // and one `#[serde(skip)]` leaves, uuid's own impls, a unit-variant
    // enum that serializes, a `Vec` of a program struct, and
    // `to_string_pretty`. Each derive's `__Field` is an item of its own.
    // The enum's `Deserialize` names `de::Unexpected<'_>`, a crate value
    // with a lifetime, which Rawr doesn't take yet, so the field skips it.
    fs::copy(root.join("programs/saved.rs"), root.join("src/main.rs")).unwrap();
    let run = rawr_import(&root, &["run"]);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        concat!(
            "{\n",
            "  \"input_value\": \"pie\",\n",
            "  \"filter\": \"Active\",\n",
            "  \"tasks\": [\n",
            "    {\n",
            "      \"id\": \"67e55044-10b1-426f-9247-bb680e5fe0c8\",\n",
            "      \"description\": \"write\",\n",
            "      \"completed\": true\n",
            "    }\n",
            "  ]\n",
            "}\n",
            "SavedState { input_value: \"pie\", filter: All, tasks: [Task { id: ",
            "67e55044-10b1-426f-9247-bb680e5fe0c8, description: \"write\", completed: true, ",
            "state: Idle }] }\n",
            "true Idle new\n",
            "\"All\"\n",
        )
    );

    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse shapes::Shape;\n\n#[derive(Debug, Shape)]\nstruct Square;\n\n\
         #[derive(Shape)]\nenum Tile {\n    Floor,\n}\n\n\
         mod round {\n    #[derive(shapes::Shape)]\n    pub struct Ring;\n}\n\n\
         fn main() {\n    #[derive(Shape)]\n    struct Circle;\n\n    \
         println!(\"{} {:?}\", Square.name(), Square);\n    \
         println!(\"{}\", Tile::Floor.name());\n    \
         println!(\"{}\", Circle.name());\n    \
         println!(\"{}\", round::Ring.name());\n}\n",
    )
    .unwrap();
    let run = rawr_import(&root, &["run"]);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "Square Square\nTile\nCircle\nRing\n");
}

/// A direct dependency that is a proc-macro crate, as `rust_decimal_macros`
/// is, is a host unit Cargo builds as a dylib, with no API to extract: a
/// native module expands its macro through Cargo's build, and a marked
/// module beside it runs. It used to fail as if the manifest changed
/// during the build, and so did a direct `cdylib` or `dylib` dependency
/// (the proc-macro issue).
#[test]
fn a_direct_proc_macro_dependency_expands_in_native_code() {
    const PROC_MACRO: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plain-proc-macro"
    );
    let root = crate::support::scratch().join("plain-proc-macro");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(PROC_MACRO), &root);

    let checked = rawr_import(&root, &["check"]);
    assert!(
        checked.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&checked.stdout),
        text(&checked.stderr)
    );
    let run = rawr_import(&root, &["run"]);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "42 value 43\n");

    // A `cdylib` dependency gets no `--extern` from Cargo, so nothing
    // names it and the check passes. A `dylib` one is an ordinary error
    // that says rawr doesn't build against it, never drift.
    fs::create_dir_all(root.join("foreign/src")).unwrap();
    fs::write(
        root.join("foreign/src/lib.rs"),
        "pub fn one() -> i64 {\n    1\n}\n",
    )
    .unwrap();
    let foreign = |kind: &str| {
        fs::write(
            root.join("foreign/Cargo.toml"),
            format!(
                "[package]\nname = \"foreign\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
                 publish = false\n\n[lib]\ncrate-type = [\"{kind}\"]\n"
            ),
        )
        .unwrap();
    };
    foreign("cdylib");
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        manifest.replace(
            "[dependencies]\n",
            "[dependencies]\nforeign = { path = \"foreign\" }\n",
        ),
    )
    .unwrap();
    let checked = rawr_import(&root, &["check"]);
    assert!(
        checked.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&checked.stdout),
        text(&checked.stderr)
    );
    foreign("dylib");
    let checked = rawr_import(&root, &["check"]);
    let stderr = text(&checked.stderr);
    assert!(!checked.status.success(), "{stderr}");
    assert!(
        stderr.contains("the dependency `foreign` is a `dylib` library"),
        "{stderr}"
    );
    assert!(!stderr.contains("changed during the build"), "{stderr}");
}

/// An edit that changes only a derive's expansion's bodies, as a new
/// `#[serde(rename)]` does, swaps into the running program (#1040): rustc
/// expands the item again, its expansion's source changes in place, and the
/// loop that serializes keeps counting.
#[test]
fn a_rename_in_a_derived_type_swaps_into_the_running_program() {
    use std::time::Duration;

    const DERIVES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-derives");
    let root = crate::support::scratch().join("plain-derives-swap");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(DERIVES), &root);
    let main = root.join("src/main.rs");
    let program = |attribute: &str| {
        format!(
            "// rawr\nuse serde::{{Deserialize, Serialize}};\n\n\
             #[derive(Debug, Serialize, Deserialize)]\nstruct Task {{\n    {attribute}\n    \
             description: String,\n}}\n\n\
             fn main() {{\n    let mut count = 0;\n    loop {{\n        \
             let task = Task {{ description: \"write\".to_string() }};\n        \
             let json = serde_json::to_string(&task).unwrap();\n        \
             let back: Task = serde_json::from_str(&json).unwrap();\n        \
             count += 1;\n        println!(\"{{count}} {{json}} {{}}\", back.description);\n        \
             std::thread::sleep(std::time::Duration::from_millis(50));\n    }}\n}}\n"
        )
    };
    fs::write(&main, program("")).unwrap();
    let mut run = crate::support::start(&root, &[]);
    let before = run.until(Duration::from_secs(900), |line| {
        line.ends_with("{\"description\":\"write\"} write")
    });
    let counted = |line: &str| line.split(' ').next().unwrap().parse::<u64>().unwrap();
    let before = counted(&before);
    fs::write(&main, program("#[serde(rename = \"text\")]")).unwrap();
    let after = run.until(Duration::from_secs(120), |line| {
        line.ends_with("{\"text\":\"write\"} write")
    });
    // The loop went on: the edit swapped in, and the program never ran again.
    assert!(counted(&after) > before, "{}", run.seen.join("\n"));
    assert!(
        !run.seen
            .iter()
            .any(|line| line.starts_with("1 ") && !line.contains("description")),
        "{}",
        run.seen.join("\n")
    );
}

/// A program's impl of a crate trait's generic method, `fn visit<V:
/// Visitor>(&self, visitor: V) -> V::Out`, which the crate calls at two
/// visitors (#593): the body reads `V` as an opaque type bounded by its
/// trait and `V::Out` as its associated type, and the runner's impl keeps
/// their values in a frame of its generic function, at each call's types.
/// So do an `Option<V>` input, a bound at the impl's lifetime (`R:
/// Reader<'a>`) over a reader that isn't `'static`, an associated function
/// of a parameter's associated type (`R::Error::custom(..)`), and a
/// borrowed program value where the crate takes `&T` (#993). A crate's
/// re-exports of std's items, as serde's `__private` module has, are those
/// items: `Formatter` and its `write_str` by path, `fmt::Result`, and the
/// variants `Some` and `None` in expressions and patterns. A crate's
/// `&dyn Trait` and `&mut dyn Trait` inputs take a value of any type that
/// implements the trait, coerced as rustc coerces it: a `&&str` through the
/// crate's impl for `&str`, a crate type, and a program type through its
/// proxy, in a free function and in a trait's provided function of an
/// opaque type, as serde's `de::Error::invalid_length` takes `&dyn
/// Expected`. The program's impl writes into the `&mut fmt::Formatter` native
/// code passes it, and passes it on to a function of its own. An opaque
/// parameter's associated type whose bound names it again, as
/// `PartialEq<Self>` of iced's `type Font: Copy + PartialEq` does, is read
/// once, not without end. A member of such a parameter's opaque type whose
/// signature names the trait's own parameter, as tracing-subscriber's
/// `Layer<S>::and_then` gives `Layered<L, Self, S>`, isn't written into the
/// crate's facade, and the rest of the crate runs (#1032). A trait's
/// provided function that takes a crate value whose type has a lifetime the
/// call picks, as serde's `de::Error::invalid_value(Unexpected<'_>, &dyn
/// Expected)` does, is the opaque `R::Error`'s, called through the trait's
/// path where the output fixes `Self`, as serde's derives call it, and
/// through the opaque type (#1031). So is a free function of such a value.
/// A parameter's projection its bounds set to the method's `Self`, as
/// image's `copy_from<O: GenericImageView<Pixel = Self::Pixel>>` sets
/// `O::Pixel`, is an opaque type of its own, not read without end, and the
/// program's impl of such a method runs; a member
/// whose closure names the trait's own parameter, as rayon's
/// `FromParallelIterator<T>::from_par_iter` gives `I::Iter` an `any` of `T`,
/// isn't written, nor are the members of a type no function gives (#1069).
#[test]
fn a_crates_generic_method_runs_the_programs_impl_at_each_type() {
    const VISITS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-visits");
    let root = crate::support::scratch().join("plain-visits");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(VISITS), &root);

    let run = rawr_import(&root, &["run"]);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        "Some(5) a pair\n14 <7> Some(10) None\n<7>\nPair(3, 4)\n1 read, expected two numbers\n\
         expected a str\nexpected 2 numbers\nexpected what I wanted\n6 3\n\
         Counts { odd: 2, even: 1 }\nodd even\n6\ntrue\n2 1\n\
         Small(2) invalid value: integer `7`, expected a number below 3\n\
         invalid value: boolean `false`, expected a number\ninteger `4`\n9 4 3\n"
    );
}

/// A crate's whole table, made by a program that called none of its
/// macros, still answers a later program's macro whose `$crate` names
/// another crate (#281): the whole walk keeps every crate's `$crate` root.
#[test]
fn a_whole_table_answers_a_later_macros_crate_root() {
    const MACROS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-macros");
    let root = crate::support::scratch().join("whole-macro-roots");
    let _ = fs::remove_dir_all(&root);
    copy(Path::new(MACROS), &root);
    let home = crate::support::scratch().join("rawr-home");
    let run = |source: &str, expected: &str| {
        fs::write(root.join("src/main.rs"), source).unwrap();
        let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(600));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), expected);
    };
    run(
        "// rawr\nfn main() {\n    println!(\"{}\", outer::__offset());\n}\n",
        "100\n",
    );
    run(
        "// rawr\nuse outer::list;\nfn main() {\n    let made = list![1, 2, 3];\n    println!(\"{}\", made.sum());\n}\n",
        // `list!` doubles each element.
        "12\n",
    );
}

/// A macro defined in the selected crate itself, whose `$crate` is that
/// crate, replays from the crate's whole table with no nightly process
/// (#404). The crates sit outside the project, so they aren't workspace
/// members and their tables are kept with the project (#398).
#[test]
fn a_whole_table_answers_the_selected_crates_own_macro() {
    const MACROS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-macros");
    let base = crate::support::scratch().join("whole-own-macro");
    let crates = base.join("crates");
    let root = base.join("app");
    let _ = fs::remove_dir_all(&base);
    for name in ["outer", "inner"] {
        copy(&Path::new(MACROS).join(name), &crates.join(name));
    }
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\n[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nouter={{path={:?}}}\n",
            crates.join("outer"),
        ),
    )
    .unwrap();
    fs::copy(
        Path::new(MACROS).join("rust-toolchain.toml"),
        root.join("rust-toolchain.toml"),
    )
    .unwrap();
    let home = crate::support::scratch().join("rawr-home");
    let run = |source: &str, expected: &str| {
        fs::write(root.join("src/main.rs"), source).unwrap();
        let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(600));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), expected);
    };
    let traces = |since: std::time::SystemTime| {
        fn walk(path: &Path, since: std::time::SystemTime, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(path).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, since, found);
                } else if !path.to_string_lossy().ends_with(".lock")
                    && entry
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .is_ok_and(|modified| modified > since)
                {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        for directory in ["build", "extraction"] {
            walk(&root.join("target/rawr").join(directory), since, &mut found);
        }
        found
    };
    run(
        "// rawr\nfn main() {\n    println!(\"{}\", outer::__offset());\n}\n",
        "100\n",
    );
    assert!(
        root.join("target/rawr/bindings").is_dir(),
        "outer's whole table is kept with the project"
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let saved = std::time::SystemTime::now();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    run(
        "// rawr\nfn main() {\n    println!(\"{} {}\", outer::__offset(), outer::offset!(5));\n}\n",
        "100 105\n",
    );
    let traces = traces(saved);
    assert_eq!(traces.len(), 0, "nightly processes ran: {traces:?}");
}

/// A project that depends on a crate the runner also needs under a name of
/// its own, as serde is for a `$crate` path, builds its runner: Cargo takes
/// one name per package, so the runner reaches the project's crate by both.
#[test]
fn a_project_dependency_the_runner_also_needs_coexists() {
    let root = crate::support::scratch().join("runner-shared-dependency");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\n[package]\nname='runner-shared-dependency'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nsemver='1'\nserde='1'\n",
    )
    .unwrap();
    fs::copy(
        Path::new(FIXTURE).join("rust-toolchain.toml"),
        root.join("rust-toolchain.toml"),
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nfn main() {\n    println!(\"{}\", semver::Version::new(1, 2, 3));\n}\n",
    )
    .unwrap();
    let home = crate::support::scratch().join("rawr-home");
    let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(600));
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "1.2.3\n");
    let manifest = fs::read_to_string(root.join("target/rawr/runner/Cargo.toml")).unwrap();
    assert_eq!(
        manifest.matches("\"serde\"").count() + manifest.matches("serde =").count(),
        1,
        "the runner depends on serde once:\n{manifest}"
    );
}

/// A save that names a new path of a path dependency, here a function,
/// replays the crates' whole tables kept with the project, after checking
/// their files: no nightly build runs (#398). A changed path crate is
/// extracted again.
#[test]
fn a_new_path_of_a_path_dependency_replays_the_projects_tables() {
    let base = crate::support::scratch().join("path-tables");
    let crates = base.join("crates");
    let root = base.join("app");
    let _ = fs::remove_dir_all(&base);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../importer/tests/fixtures");
    for name in ["a", "b", "c"] {
        copy(
            &fixtures.join(format!("applicability-{name}")),
            &crates.join(format!("applicability-{name}")),
        );
    }
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\n[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-a={{path={:?}}}\nfacts-c={{path={:?}}}\n",
            crates.join("applicability-a"),
            crates.join("applicability-c"),
        ),
    )
    .unwrap();
    fs::copy(
        Path::new(FIXTURE).join("rust-toolchain.toml"),
        root.join("rust-toolchain.toml"),
    )
    .unwrap();
    let home = crate::support::scratch().join("rawr-home");
    let run = |source: &str, expected: &str| {
        fs::write(root.join("src/main.rs"), source).unwrap();
        let run = rawr_import_within(&root, &["run"], &home, std::time::Duration::from_secs(600));
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), expected);
    };
    // The nightly check build and its extraction leave files where they run.
    let nightly = |since: std::time::SystemTime| {
        fn walk(path: &Path, since: std::time::SystemTime, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(path).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, since, found);
                } else if !path.to_string_lossy().ends_with(".lock")
                    && entry
                        .metadata()
                        .and_then(|metadata| metadata.modified())
                        .is_ok_and(|modified| modified > since)
                {
                    found.push(path);
                }
            }
        }
        let mut found = Vec::new();
        for directory in ["build", "extraction"] {
            walk(&root.join("target/rawr").join(directory), since, &mut found);
        }
        found
    };
    let bound =
        "// rawr\nfn main() {\n    let _bound = facts_a::left();\n    println!(\"{}\", 1);\n}\n";
    run(bound, "1\n");
    run(bound, "1\n");
    assert!(
        root.join("target/rawr/bindings").is_dir(),
        "the path crates' tables are kept with the project"
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let saved = std::time::SystemTime::now();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    run(
        "// rawr\nfn main() {\n    let _bound = facts_a::left();\n    let both = facts_a::both();\n    let _ = both;\n    println!(\"{}\", 2);\n}\n",
        "2\n",
    );
    let traces = nightly(saved);
    assert_eq!(traces.len(), 0, "nightly processes ran: {traces:?}");
    assert!(
        !home
            .join("bindings")
            .read_dir()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                fs::read_to_string(entry.path().join("meta.json"))
                    .is_ok_and(|meta| meta.contains("\"crate_name\":\"facts_a\""))
            }),
        "a path crate never enters rawr's home"
    );
}

/// A cargo script that declares a dependency runs as `cargo -Zscript`
/// runs it (D608 point 6). Nothing is kept beside it: its manifest and
/// lockfile are in rawr's area, keyed by its path, and a second script
/// with the same dependency reuses the crate cargo compiled for the first.
#[test]
fn a_script_with_a_dependency_runs_from_its_frontmatter() {
    let directory = crate::support::scratch().join("plain-scripts");
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let script = |name: &str, line: &str| {
        let path = directory.join(name);
        fs::write(
            &path,
            format!(
                "#!/usr/bin/env -S cargo +nightly -Zscript\n---cargo\n[dependencies]\n\
                 semver = \"1\"\n---\n// rawr\nuse semver::Version;\n\nfn main() {{\n    \
                 let version = Version::parse(\"1.2.3\").unwrap();\n    {line}\n}}\n"
            ),
        )
        .unwrap();
        path
    };
    let first = script("parse.rs", "println!(\"parsed: {version}\");");
    let second = script("major.rs", "println!(\"major: {}\", version.major);");

    let run = rawr_import(&first, &["run"]);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "parsed: 1.2.3\n");
    let run = rawr_import(&second, &["run"]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(text(&run.stdout), "major: 1\n");

    let mut beside = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    beside.sort();
    assert_eq!(beside, ["major.rs", "parse.rs"]);
    let area = crate::support::scratch().join("rawr-home/scripts");
    for stem in ["parse", "major"] {
        let own = fs::read_dir(&area)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&format!("{stem}-"))
            })
            .unwrap_or_else(|| panic!("no directory for {stem}.rs in {}", area.display()));
        let lock = fs::read_to_string(own.join("Cargo.lock")).unwrap();
        assert!(lock.contains("name = \"semver\""), "{lock}");
    }
    let semver = fs::read_dir(area.join("build/debug/deps"))
        .unwrap()
        .filter(|entry| {
            let name = entry.as_ref().unwrap().file_name();
            let name = name.to_string_lossy();
            name.starts_with("libsemver-") && name.ends_with(".rlib")
        })
        .count();
    assert_eq!(semver, 1);
}

/// A project that shares a dependency with rawr's runtime at fewer
/// features: ahash, which rawr takes with `compile-time-rng` and so with
/// `const-random`. The runner's lockfile is what cargo resolves once rawr
/// is gone, so the runner's `--locked` build runs, and a second run keeps
/// that lockfile.
#[test]
fn a_dependency_rawr_takes_with_more_features_keeps_the_locked_runner_build() {
    const SHARED: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/plain-shared-feature"
    );
    let root = crate::support::scratch().join("plain-shared-feature");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(SHARED), &root);

    for _ in 0..2 {
        let run = rawr_import(&root, &["run"]);
        assert!(
            run.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            text(&run.stdout),
            text(&run.stderr)
        );
        assert_eq!(text(&run.stdout), "seeded\n");
    }
    let lock = fs::read_to_string(root.join("target/rawr/runner/Cargo.lock")).unwrap();
    assert!(lock.contains("name = \"ahash\""), "{lock}");
    assert!(!lock.contains("const-random"), "{lock}");
    let main = fs::read_to_string(root.join("target/rawr/runner/src/main.rs")).unwrap();
    assert!(
        main.contains("extern crate rawr as __rawr_internal;"),
        "{main}"
    );
}

/// Write `files` under a fresh `root`, with the fixture's toolchain.
fn project(root: &Path, files: &[(&str, &str)]) {
    let _ = fs::remove_dir_all(root);
    for (path, source) in files {
        let destination = root.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, source).unwrap();
    }
    fs::copy(
        Path::new(FIXTURE).join("rust-toolchain.toml"),
        root.join("rust-toolchain.toml"),
    )
    .unwrap();
}

/// A prerelease no registry publishes, which only the patch provides: an
/// unpatched resolution fails as Cargo's does.
const PATCHED_GAUGE: [(&str, &str); 2] = [
    (
        "gauge/Cargo.toml",
        "[package]\nname='rawr-patched-gauge'\nversion='0.9.0-dev'\nedition='2024'\n",
    ),
    ("gauge/src/lib.rs", "pub fn sides() -> i64 { 6 }\n"),
];

const PATCHED_MAIN: &str =
    "// rawr\nfn main() {\n    println!(\"sides: {}\", rawr_patched_gauge::sides());\n}\n";

/// Run and check `program`, and compare its check with `cargo check`'s.
fn assert_runs_and_checks(program: &Path, home: &Path) {
    let limit = std::time::Duration::from_secs(600);
    let run = rawr_import_within(program, &["run"], home, limit);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), "sides: 6\n");
    let checked = rawr_import_within(program, &["check"], home, limit);
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert_eq!(
        text(&checked.stdout),
        format!(
            "Checked {}\ncargo check passed\n",
            program.join("src/main.rs").display()
        )
    );
}

/// A registry dependency that the package's own `[patch.crates-io]`
/// replaces with a path crate resolves to the patch, as under Cargo (#668).
#[test]
fn a_registry_dependency_patched_to_a_path_resolves_to_the_patch() {
    let root = crate::support::scratch().join("patched-dependency");
    let mut files = PATCHED_GAUGE.to_vec();
    files.extend([
        (
            "Cargo.toml",
            "[workspace]\n[package]\nname='patched-dependency'\nversion='0.1.0'\nedition='2024'\n\
             [dependencies]\nrawr-patched-gauge='0.9.0-dev'\n\
             [patch.crates-io]\nrawr-patched-gauge={path='gauge'}\n",
        ),
        ("src/main.rs", PATCHED_MAIN),
    ]);
    project(&root, &files);
    let home = crate::support::scratch().join("rawr-home");
    assert_runs_and_checks(&root, &home);
}

/// `Sides::from(sides)` of a crate type is core's reflexive `impl<T>
/// From<T> for T`, beside the crate's own `From` impls, as `column!`'s
/// `Element::from(element)` is. `Sides::from(6)` beside it is the crate's
/// `From<i64>`.
#[test]
fn a_crate_types_from_of_itself_gives_the_value_back() {
    let root = crate::support::scratch().join("reflexive-from");
    project(
        &root,
        &[
            (
                "Cargo.toml",
                "[workspace]\n[package]\nname='reflexive-from'\nversion='0.1.0'\nedition='2024'\n\
                 [dependencies]\nsides={path='sides'}\n",
            ),
            (
                "sides/Cargo.toml",
                "[package]\nname='sides'\nversion='0.1.0'\nedition='2024'\n",
            ),
            (
                "sides/src/lib.rs",
                "pub struct Sides(i64);\n\
                 impl Sides { pub fn count(&self) -> i64 { self.0 } }\n\
                 impl From<i64> for Sides { fn from(count: i64) -> Self { Sides(count) } }\n\
                 impl From<&str> for Sides { fn from(name: &str) -> Self { Sides(name.len() as i64) } }\n",
            ),
            (
                "src/main.rs",
                "// rawr\nuse sides::Sides;\nfn main() {\n    let sides = Sides::from(6);\n    \
                 let same = Sides::from(sides);\n    println!(\"sides: {}\", same.count());\n}\n",
            ),
        ],
    );
    let home = crate::support::scratch().join("rawr-home");
    assert_runs_and_checks(&root, &home);
}

/// A workspace member resolves with the workspace root's `[patch]`, as
/// Cargo resolves every member (#664).
#[test]
fn a_workspace_member_resolves_with_the_roots_patch() {
    let root = crate::support::scratch().join("patched-workspace");
    let mut files = PATCHED_GAUGE.to_vec();
    files.extend([
        (
            "Cargo.toml",
            "[workspace]\nmembers=['app']\nresolver='3'\n\
             [patch.crates-io]\nrawr-patched-gauge={path='gauge'}\n",
        ),
        (
            "app/Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\nedition='2024'\n\
             [dependencies]\nrawr-patched-gauge='0.9.0-dev'\n",
        ),
        ("app/src/main.rs", PATCHED_MAIN),
    ]);
    project(&root, &files);
    let home = crate::support::scratch().join("rawr-home");
    assert_runs_and_checks(&root.join("app"), &home);
}

/// A project as `FIXTURE`, with `main` and, when `events` is given, a path
/// crate `events 0.1.0` at `events/` that returns it.
fn nightly_project(root: &Path, main: &str, events: Option<&str>) {
    let _ = fs::remove_dir_all(root);
    copy(Path::new(FIXTURE), root);
    fs::write(root.join("src/main.rs"), main).unwrap();
    if let Some(word) = events {
        fs::create_dir_all(root.join("events/src")).unwrap();
        fs::write(
            root.join("events/Cargo.toml"),
            "[package]\nname = \"events\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            root.join("events/src/lib.rs"),
            format!("pub fn word() -> &'static str {{\n    \"{word}\"\n}}\n"),
        )
        .unwrap();
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            manifest.replace(
                "semver = \"1\"\n",
                "semver = \"1\"\nevents = { path = \"events\" }\n",
            ),
        )
        .unwrap();
    }
}

fn run_ok(root: &Path, home: &Path) -> String {
    let run = rawr_import_with_home(root, &["run"], home);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    text(&run.stdout)
}

/// Two projects with a path crate `events 0.1.0` at the same relative path,
/// with different contents, share the pinned nightly's check build. Cargo
/// keys each `events` by its absolute path, so each project reads its own,
/// the second compiles no registry unit, and neither removes the other's
/// units (D699.2 a and c, D700).
#[test]
fn a_same_named_path_crate_of_another_project_never_stands_in_for_this_ones() {
    let base = crate::support::scratch().join("nightly-path-crates");
    let _ = fs::remove_dir_all(&base);
    let home = base.join("home");
    let nightly = nightly_directory(&home);
    let main = "// rawr\nuse semver::Version;\n\nfn main() {\n    println!(\"{} {}\", events::word(), Version::new(1, 2, 3));\n}\n";
    let [first, second] = ["first", "second"].map(|name| base.join(name));
    nightly_project(&first, main, Some("alpha"));
    nightly_project(&second, main, Some("beta"));

    assert_eq!(run_ok(&first, &home), "alpha 1.2.3\n");
    assert!(nightly_built(&first));
    assert!(
        !units_of(&nightly, "semver").is_empty(),
        "the first project checks semver"
    );

    let started = std::time::SystemTime::now();
    assert_eq!(run_ok(&second, &home), "beta 1.2.3\n");
    assert!(
        nightly_built(&second),
        "the second project extracts its own `events`"
    );
    let compiled = compiled_since(&nightly, started);
    assert!(
        compiled.iter().any(|unit| unit.starts_with("events-")),
        "the second project's `events` is its own unit: {compiled:?}"
    );
    assert!(
        compiled
            .iter()
            .all(|unit| unit.starts_with("events-") || unit.starts_with("rawr-resolution-harness-")),
        "the second project compiled only its own crates: {compiled:?}"
    );
    assert_eq!(
        units_of(&nightly, "events").len(),
        2,
        "each project keeps its own `events` unit"
    );

    // The first project's `events` is still its own, and still fresh.
    fs::remove_dir_all(home.join("bindings")).unwrap();
    fs::remove_dir_all(first.join("target/rawr")).unwrap();
    let started = std::time::SystemTime::now();
    assert_eq!(run_ok(&first, &home), "alpha 1.2.3\n");
    assert!(nightly_built(&first));
    // Only the harness, whose sources rawr has just written, is checked again.
    let compiled = compiled_since(&nightly, started);
    assert!(
        compiled
            .iter()
            .all(|unit| unit.starts_with("rawr-resolution-harness-")),
        "{compiled:?}"
    );
}

/// Two projects with the same closure and no path crate share every unit
/// of the nightly's check build, the harness's included. A binding miss in
/// the second compiles nothing and reads what the first checked, which is
/// what it would check itself; a feature change compiles only that variant,
/// beside the first's (D699.2 b, D700).
#[test]
fn identical_closures_share_the_nightly_build_and_read_no_stale_results() {
    let base = crate::support::scratch().join("nightly-identical");
    let _ = fs::remove_dir_all(&base);
    let home = base.join("home");
    let nightly = nightly_directory(&home);
    let main = "// rawr\nuse semver::Version;\n\nfn main() {\n    let version = Version::parse(\"1.2.3\").unwrap();\n    println!(\"{version} {}\", version.major);\n}\n";
    let [first, second] = ["first", "second"].map(|name| base.join(name));
    nightly_project(&first, main, None);
    nightly_project(&second, main, None);

    assert_eq!(run_ok(&first, &home), "1.2.3 1\n");
    assert!(nightly_built(&first));

    // A miss in the second project, as after a change to the bindings'
    // format: the nightly runs and compiles nothing.
    fs::remove_dir_all(home.join("bindings")).unwrap();
    let started = std::time::SystemTime::now();
    assert_eq!(run_ok(&second, &home), "1.2.3 1\n");
    assert!(
        nightly_built(&second),
        "the second project's miss ran the nightly"
    );
    // Only the harness, whose sources rawr has just written, is checked again.
    let compiled = compiled_since(&nightly, started);
    assert!(
        compiled
            .iter()
            .all(|unit| unit.starts_with("rawr-resolution-harness-")),
        "{compiled:?}"
    );

    // Another feature set is another unit: only it is compiled, and the
    // first project's stays.
    let before = units_of(&nightly, "semver");
    let manifest = fs::read_to_string(second.join("Cargo.toml")).unwrap();
    fs::write(
        second.join("Cargo.toml"),
        manifest.replace(
            "semver = \"1\"\n",
            "semver = { version = \"1\", default-features = false }\n",
        ),
    )
    .unwrap();
    let started = std::time::SystemTime::now();
    assert_eq!(run_ok(&second, &home), "1.2.3 1\n");
    let compiled = compiled_since(&nightly, started);
    assert!(
        compiled.iter().any(|unit| unit.starts_with("semver-")),
        "semver without `std` is a unit of its own: {compiled:?}"
    );
    assert!(
        compiled
            .iter()
            .all(|unit| unit.starts_with("semver-") || unit.starts_with("rawr-resolution-harness-")),
        "{compiled:?}"
    );
    let semver = units_of(&nightly, "semver");
    assert!(
        before.iter().all(|unit| semver.contains(unit)) && semver.len() > before.len(),
        "both variants stay: {before:?} then {semver:?}"
    );
}
