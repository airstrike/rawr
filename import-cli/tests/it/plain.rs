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
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rawr-home");
    rawr_import_with_home(root, arguments, &home)
}

fn rawr_import_with_home(root: &Path, arguments: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .args(arguments)
        .arg(root)
        .env("RAWR_HOME", home)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
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
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    let stdout = directory.join(format!("{name}.stdout"));
    let stderr = directory.join(format!("{name}.stderr"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .args(arguments)
        .arg(root)
        .env("RAWR_HOME", home)
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-dependency");
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("concrete-trait-arguments");
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rawr-home");
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
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lexical-applications");
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
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("new-member-no-nightly");
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
        for directory in ["build", "extraction", "cache/resolution", "inventories"] {
            walk(&root.join("target/rawr").join(directory), since, &mut found);
        }
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
    run("11\n");
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
/// back instead of publishing its own. Path crates keep theirs per project.
#[test]
fn an_immutable_crates_intrinsic_entry_is_shared_per_machine() {
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shared-inventories");
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
                "[workspace]\n[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nfacts-a={{path={:?}}}\nfacts-c={{path={:?}}}\nsemver='1'\n",
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
        ["semver"],
        "only the registry crate's entry is machine-wide"
    );
    let local = entries(&first.join("target/rawr/inventories"));
    assert_eq!(
        local
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["facts_a", "facts_b", "facts_c"]
    );
    let second = project("second");
    assert_eq!(
        entries(&home.join("inventories")),
        shared,
        "the second project reads the shared entry back"
    );
    assert_eq!(entries(&second.join("target/rawr/inventories")).len(), 3);
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
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("semver-sharing-home");
    let bindings = home.join("bindings");
    let _ = fs::remove_dir_all(&bindings);
    let projects = ["first", "second", "other", "other-again"]
        .map(|name| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("shared-{name}")));
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
        root.join("target/rawr/build").exists() || probed != 0
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

/// The declared inputs of a shared immutable crate are checked before both
/// `check` and a remembered warm run consume its API. The unchanged path still
/// shares across projects without a nightly build.
#[test]
fn shared_git_bindings_validate_declared_file_and_environment_before_replay() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shared-script-inputs");
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
            .env(
                "RAWR_HOME",
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rawr-home"),
            )
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
            !project.join("target/rawr/build").exists(),
            "unchanged crate should replay without nightly: {}",
            project.display()
        );
        let output = run(project, "run", "small");
        assert!(output.status.success(), "{}", text(&output.stderr));
        assert!(!project.join("target/rawr/build").exists());
        assert!(project.join("target/rawr/shared/remembered.json").is_file());
    }
    let checked = run(&projects[1], "check", "small");
    assert!(checked.status.success(), "{}", text(&checked.stderr));
    assert!(!projects[1].join("target/rawr/build").exists());
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
    assert!(!live.join("target/rawr/build").exists());
    finish(&mut shared);
    assert!(live.join("target/rawr/shared/remembered.json").is_file());
    let mut remembered = start();
    remembered.until(std::time::Duration::from_secs(120), |line| {
        line == "width 10"
    });
    assert!(!live.join("target/rawr/build").exists());
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shared-adapter-drift");
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
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rawr-home");
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-arguments");
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-macros");
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

/// A cargo script that declares a dependency runs as `cargo -Zscript`
/// runs it (D608 point 6). Nothing is kept beside it: its manifest and
/// lockfile are in rawr's area, keyed by its path, and a second script
/// with the same dependency reuses the crate cargo compiled for the first.
#[test]
fn a_script_with_a_dependency_runs_from_its_frontmatter() {
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-scripts");
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
    let area = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("rawr-home/scripts");
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
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plain-shared-feature");
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
