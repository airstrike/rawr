use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn check_accepts_a_one_file_counter() {
    let source = temporary_source("counter");
    fs::write(
        &source,
        include_str!("../../../iced/examples/scripts/application_counter.rs"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rawr-iced"))
        .args(["check", source.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Checked"));
    fs::remove_file(source).unwrap();
}

#[test]
fn check_reports_compile_failure_with_a_nonzero_exit() {
    let source = temporary_source("broken");
    fs::write(&source, "this is not a Rawr application").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rawr-iced"))
        .args(["check", source.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error["));
    fs::remove_file(source).unwrap();
}

#[test]
fn help_version_and_unknown_command_have_concise_exits() {
    for argument in ["-h", "--help", "help", "-V", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rawr-iced"))
            .arg(argument)
            .output()
            .unwrap();
        assert!(output.status.success(), "{argument}");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_rawr-iced"))
        .arg("rn")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command `rn`"));
}

#[test]
fn new_creates_the_exact_marked_crate_in_a_missing_target() {
    let sandbox = Sandbox::new("new-missing");
    let target = sandbox.path().join("counter");

    let output = rawr_command()
        .args(["new", target.to_str().unwrap()])
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(
        fs::read_to_string(target.join("src/main.rs")).unwrap(),
        rawr_iced_cli::new::COUNTER_SOURCE
    );
    assert_eq!(
        fs::read_to_string(target.join("Cargo.toml")).unwrap(),
        "[package]\nname = \"counter\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [dependencies]\niced = { git = \"https://github.com/iced-rs/iced\", \
         rev = \"be88873d38544455e26c7a104eb757da2cbbac6b\" }\n"
    );
    assert!(!target.join("rawr.toml").exists());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&target.display().to_string()));
    assert!(stdout.contains("rawr check"));
    assert!(stdout.contains("rawr run"));
}

#[test]
fn new_accepts_a_verified_empty_existing_directory() {
    let sandbox = Sandbox::new("new-empty");
    let target = sandbox.path().join("counter");
    fs::create_dir(&target).unwrap();

    let created = rawr_iced_cli::new::create(target.clone()).unwrap();

    assert_eq!(created.root(), target);
    assert_eq!(
        fs::read_to_string(target.join("src/main.rs")).unwrap(),
        rawr_iced_cli::new::COUNTER_SOURCE
    );
}

#[test]
fn new_refuses_non_empty_targets_without_changing_a_byte() {
    let sandbox = Sandbox::new("new-non-empty");
    let target = sandbox.path().join("counter");
    fs::create_dir(&target).unwrap();
    let existing = target.join("keep.bin");
    let original = b"\0user-owned\xffbytes";
    fs::write(&existing, original).unwrap();

    let error = rawr_iced_cli::new::create(target.clone()).unwrap_err();

    assert!(error.to_string().contains("non-empty"));
    assert_eq!(fs::read(&existing).unwrap(), original);
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
}

#[test]
fn new_refuses_existing_files_and_repeated_initialization() {
    let sandbox = Sandbox::new("new-existing");
    let file_target = sandbox.path().join("counter-file");
    let original = b"not a project";
    fs::write(&file_target, original).unwrap();

    let file_error = rawr_iced_cli::new::create(file_target.clone()).unwrap_err();
    assert!(file_error.to_string().contains("existing file"));
    assert_eq!(fs::read(&file_target).unwrap(), original);

    let project_target = sandbox.path().join("counter-project");
    rawr_iced_cli::new::create(project_target.clone()).unwrap();
    let starter = fs::read(project_target.join("src/main.rs")).unwrap();
    let repeated = rawr_iced_cli::new::create(project_target.clone()).unwrap_err();
    assert!(repeated.to_string().contains("non-empty"));
    assert_eq!(
        fs::read(project_target.join("src/main.rs")).unwrap(),
        starter
    );
}

#[cfg(unix)]
#[test]
fn new_refuses_a_target_symlink_without_following_it() {
    use std::os::unix::fs::symlink;

    let sandbox = Sandbox::new("new-symlink");
    let destination = sandbox.path().join("destination");
    let target = sandbox.path().join("counter");
    fs::create_dir(&destination).unwrap();
    symlink(&destination, &target).unwrap();

    let error = rawr_iced_cli::new::create(target.clone()).unwrap_err();

    assert!(error.to_string().contains("target symlink"));
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    assert!(fs::symlink_metadata(&target)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn new_refuses_a_symlink_in_the_existing_ancestor_chain() {
    use std::os::unix::fs::symlink;

    let sandbox = Sandbox::new("new-ancestor-symlink");
    let destination = sandbox.path().join("destination");
    let linked_parent = sandbox.path().join("linked-parent");
    fs::create_dir(&destination).unwrap();
    symlink(&destination, &linked_parent).unwrap();

    let error = rawr_iced_cli::new::create(linked_parent.join("counter")).unwrap_err();

    assert!(error.to_string().contains("ancestor"));
    assert!(error.to_string().contains("symlink"));
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
}

#[test]
fn new_resolves_nested_relative_paths_and_the_result_checks() {
    let sandbox = Sandbox::new("new-nested-check");

    let created = rawr_command()
        .current_dir(sandbox.path())
        .args(["new", "work/examples/counter"])
        .output()
        .unwrap();
    assert_success(&created);

    let target = sandbox.path().join("work/examples/counter");
    let checked = rawr_command()
        .args(["check", target.to_str().unwrap()])
        .output()
        .unwrap();
    assert_success(&checked);
    assert!(String::from_utf8_lossy(&checked.stdout).contains("Checked"));
}

#[test]
fn packaged_starter_cannot_drift_from_the_runnable_golden_counter() {
    assert_eq!(
        rawr_iced_cli::new::COUNTER_SOURCE,
        include_str!("../../../iced/examples/scripts/application_counter.rs")
    );
}

#[test]
fn snapshot_update_check_and_missing_baseline_are_end_to_end() {
    let sandbox = Sandbox::new("snapshot-lifecycle");
    let project = sandbox.path().join("counter");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("src/main.rs"),
        include_str!("../../../iced/examples/package_snapshots/src/main.rs"),
    )
    .unwrap();
    fs::write(
        project.join("Cargo.toml"),
        include_str!("../../../iced/examples/package_snapshots/Cargo.toml"),
    )
    .unwrap();

    let updated = rawr_command()
        .args(["snapshot", "--update", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_success(&updated);
    assert_eq!(updated.stdout, b"Updated 5 snapshots.\n");

    let baseline = project.join("snapshots");
    let before = tree(&baseline);
    let checked = rawr_command()
        .args(["snapshot", "--check", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_success(&checked);
    assert_eq!(checked.stdout, b"5 snapshots match.\n");
    assert_eq!(tree(&baseline), before, "check must not mutate baselines");

    let verbose_match = rawr_command()
        .args([
            "snapshot",
            "--check",
            "--verbose",
            project.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_success(&verbose_match);
    assert_eq!(
        String::from_utf8_lossy(&verbose_match.stdout),
        format!(
            "5 snapshots match.\nBaseline: {}/snapshots\nRendered: {}/target/rawr/snapshots\n",
            project.display(),
            project.display(),
        )
    );

    let removed = before
        .keys()
        .find(|path| path.extension().is_some_and(|extension| extension == "png"))
        .map(|path| baseline.join(path))
        .unwrap();
    fs::remove_file(&removed).unwrap();
    let drift = rawr_command()
        .args(["snapshot", "--check", project.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(drift.status.code(), Some(1));
    assert!(drift.stderr.is_empty());
    let drift_stdout = String::from_utf8_lossy(&drift.stdout);
    assert!(drift_stdout.contains("[missing baseline]"));
    assert!(drift_stdout.contains("snapshot_counter::Counter::view#default"));
    assert!(drift_stdout.contains("expected:"));
    assert!(drift_stdout.contains("actual:"));
    assert!(drift_stdout.contains("expected: snapshots/"));
    assert!(drift_stdout.contains("actual:   target/rawr/snapshots/"));
    assert!(!drift_stdout.contains(&project.display().to_string()));
    assert!(!drift_stdout.contains("Actual artifacts"));

    let verbose = rawr_command()
        .args([
            "snapshot",
            "--check",
            "--verbose",
            project.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(verbose.status.code(), Some(1));
    assert!(verbose.stderr.is_empty());
    let verbose_stdout = String::from_utf8_lossy(&verbose.stdout);
    assert!(verbose_stdout.contains(&format!("expected: {}/snapshots/", project.display())));
    assert!(verbose_stdout.contains(&format!(
        "actual:   {}/target/rawr/snapshots/",
        project.display()
    )));
    assert!(verbose_stdout.contains(&format!("Baseline: {}/snapshots", project.display())));
    assert!(verbose_stdout.contains(&format!(
        "Rendered: {}/target/rawr/snapshots",
        project.display()
    )));
    assert!(!verbose_stdout.contains("Actual artifacts"));
}

#[test]
fn check_and_run_share_byte_identical_golden_source_diagnostics() {
    for fixture in [
        "syntax",
        "duplicate_item",
        "unknown_type",
        "unknown_callback",
    ] {
        let source = fixture_path(fixture, "rs");
        let expected = fs::read_to_string(fixture_path(fixture, "stderr")).unwrap();
        let checked = rawr_command()
            .args(["check", source.to_str().unwrap()])
            .output()
            .unwrap();
        let run = rawr_command()
            .args(["run", source.to_str().unwrap()])
            .output()
            .unwrap();

        assert!(!checked.status.success(), "{fixture} unexpectedly checked");
        assert!(!run.status.success(), "{fixture} unexpectedly ran");
        assert_eq!(checked.stderr, run.stderr, "{fixture} command drift");
        assert_eq!(
            normalize(&checked.stderr, &[(source.as_path(), "$SOURCE")]),
            expected,
            "{fixture} golden drift"
        );
    }
}

#[test]
fn check_and_run_share_exact_child_module_diagnostics() {
    let sandbox = Sandbox::new("child-module-diagnostic");
    let project = sandbox.path().join("counter");
    fs::create_dir_all(project.join("src")).unwrap();
    write_crate(&project);
    fs::write(project.join("src/main.rs"), "// rawr\npub mod broken;\n").unwrap();
    fs::write(
        project.join("src/broken.rs"),
        "// rawr\npub fn wrong(value: i64) -> bool { value }\n",
    )
    .unwrap();

    let checked = rawr_command()
        .args(["check", project.to_str().unwrap()])
        .output()
        .unwrap();
    let run = rawr_command()
        .args(["run", project.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!checked.status.success());
    assert!(!run.status.success());
    // Rawr's diagnostic is the same; cargo's errors follow it in `check`
    // and come from the background check in `run`.
    let checked = String::from_utf8_lossy(&checked.stderr);
    let rawr = checked.split("\ncargo check").next().unwrap();
    assert!(rawr.contains("src/broken.rs"), "{checked}");
    assert!(String::from_utf8_lossy(&run.stderr).contains(rawr));
}

#[test]
fn project_discovery_failure_uses_the_environment_golden() {
    let sandbox = Sandbox::new("diagnostic-not-found");
    let input = sandbox.path().join("missing.rs");
    let output = rawr_command()
        .args(["check", input.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(
        normalize(&output.stderr, &[(input.as_path(), "$INPUT")]),
        include_str!("../fixtures/diagnostics/input_not_found.stderr")
    );
}

#[test]
fn doctor_reads_broken_source_without_compiling_it() {
    let sandbox = Sandbox::new("doctor-no-compile");
    let project = sandbox.path().join("counter");
    fs::create_dir_all(project.join("src")).unwrap();
    write_crate(&project);
    fs::write(
        project.join("src/main.rs"),
        "// rawr\nthis is deliberately not a valid Rawr application",
    )
    .unwrap();

    let output = rawr_command()
        .args(["doctor", project.to_str().unwrap()])
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[OK] source: readable UTF-8"));
    assert!(!stdout.contains("RAWR-PARSE"));
    assert!(!stdout.contains("RAWR-RESOLVE"));
}

#[test]
fn doctor_warnings_do_not_fail_when_no_project_is_present() {
    let sandbox = Sandbox::new("doctor-no-project");

    let output = rawr_command()
        .current_dir(sandbox.path())
        .arg("doctor")
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[OK] versions:"));
    assert!(stdout.contains("[OK] renderer:"));
    assert!(stdout.contains("[WARN] project:"));
    assert!(stdout.contains("[OK] extensions:"));
}

#[test]
fn doctor_explicit_failure_reports_every_check_before_exit_one() {
    let sandbox = Sandbox::new("doctor-explicit-failure");
    let missing = sandbox.path().join("missing.rs");

    let output = rawr_command()
        .args(["doctor", missing.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[OK] versions:"));
    assert!(stdout.contains("[FAIL] project:"));
    assert!(stdout.contains("[WARN] source:"));
    assert!(stdout.contains("[OK] extensions:"));
}

#[test]
fn doctor_extension_locations_are_future_ready_warnings() {
    let sandbox = Sandbox::new("doctor-extensions");
    let missing = sandbox.path().join("missing-extensions");

    let output = rawr_command()
        .current_dir(sandbox.path())
        .arg("doctor")
        .env("RAWR_EXTENSION_PATH", &missing)
        .output()
        .unwrap();

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[WARN] extensions:"));
    assert!(stdout.contains("fix RAWR_EXTENSION_PATH"));
    assert!(stdout.contains(&missing.display().to_string()));
}

fn fixture_path(stem: &str, extension: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/diagnostics")
        .join(format!("{stem}.{extension}"))
}

fn normalize(output: &[u8], replacements: &[(&Path, &str)]) -> String {
    let mut output = String::from_utf8(output.to_vec()).unwrap();
    for (path, replacement) in replacements {
        output = output.replace(&path.display().to_string(), replacement);
    }
    output
}

fn tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn visit(
        root: &Path,
        directory: &Path,
        files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }

    let mut files = std::collections::BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn rawr_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rawr-iced"))
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Sandbox {
    path: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let path = temporary_path(label);
        fs::create_dir(&path).unwrap();
        Self {
            path: fs::canonicalize(path).unwrap(),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            panic!(
                "failed to clean test sandbox `{}`: {error}",
                self.path.display()
            );
        }
    }
}

fn temporary_source(label: &str) -> std::path::PathBuf {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rawr-cli-{label}-{}-{id}.rs", std::process::id()))
}

/// A Cargo manifest for the crate at `root`, with its `src` directory.
fn write_crate(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
}

fn temporary_path(label: &str) -> PathBuf {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("rawr-cli-{label}-{}-{id}", std::process::id()))
}

/// `rawr` hands an iced program to rawr-iced after parsing it once: what
/// `rawr` does in its own process (load, then keep the parse cells) and
/// then rawr-iced, started as `rawr` starts it, reading every source from
/// those cells. Each source is parsed once across both processes.
#[test]
fn an_iced_program_handed_from_rawr_to_rawr_iced_is_parsed_once() {
    let sandbox = Sandbox::new("parse-once");
    let root = sandbox.path().to_path_buf();
    write_crate(&root);
    // `rawr check` runs `cargo check` too, which needs iced.
    fs::write(
        root.join("Cargo.toml"),
        rawr_iced_cli::new::manifest("parse-once"),
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        include_str!("../../../iced/examples/scripts/application_counter.rs")
            .replacen("// rawr\n", "// rawr\nmod parts;\n", 1),
    )
    .unwrap();
    fs::write(
        root.join("src/parts.rs"),
        "// rawr\npub fn label() -> String {\n    String::from(\"count\")\n}\n",
    )
    .unwrap();

    // `rawr`: one load, and the program goes to rawr-iced.
    let loaded = rawr_cli::base::load(Some(&root)).unwrap();
    assert!(rawr_iced_cli::layer::claims().claim(loaded.sheet.facts()));
    assert_eq!(loaded.cells.counts().parsed, 2);
    rawr_cli::cells::keep("rawr", &loaded.cells, &loaded.graph);
    drop(loaded);

    // rawr-iced, with the arguments `rawr` hands it.
    let output = rawr_command()
        .args(["check"])
        .arg(&root)
        .env("RAWR_VERBOSE", "1")
        .output()
        .unwrap();

    assert_success(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rawr-iced: parsed 0 sources, read 2 from parse cells"),
        "{stderr}"
    );
}

/// The iced counter as a Cargo crate of two `.rs` files marked `// rawr`,
/// without its native module: rawr-iced serves it from one parse of each
/// file.
#[test]
fn a_marked_iced_crate_is_served_by_rawr_iced() {
    let sandbox = Sandbox::new("marked-counter");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cli/tests/fixtures/rs-marker-counter");
    let root = sandbox.path().join("counter");
    fs::create_dir_all(root.join("src")).unwrap();
    for file in ["Cargo.toml", "src/main.rs", "src/ui.rs"] {
        let text = fs::read_to_string(fixture.join(file)).unwrap();
        let text = text
            .replace("mod theme;\n", "")
            .replace("use crate::theme::format_count;\n", "")
            .replace("format_count(counter.value)", "counter.value");
        fs::write(root.join(file), text).unwrap();
    }

    let loaded = rawr_cli::base::load(Some(&root)).unwrap();
    assert!(rawr_iced_cli::layer::claims().claim(loaded.sheet.facts()));
    assert_eq!(loaded.cells.counts().parsed, 2);
    assert!(!loaded.graph.declares_rust_import());
    drop(loaded);

    let prepared = rawr_iced_cli::run::prepare(Some(root)).unwrap();
    assert!(prepared.adapter().last_good_output().is_some());
    assert!(prepared.adapter().diagnostics().is_empty());
}

/// Previews written for rustc through the `rawr` attribute crate:
/// `#[preview]` after `use rawr::preview;`, and `#[rawr::preview(..)]`.
/// rawr-iced serves both, and the crate's dependency imports nothing.
#[test]
fn a_marked_iced_crate_writes_previews_through_the_rawr_crate() {
    let sandbox = Sandbox::new("marked-previews");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../cli/tests/fixtures/rs-marker-counter");
    let root = sandbox.path().join("counter");
    fs::create_dir_all(root.join("src")).unwrap();
    for file in ["Cargo.toml", "src/main.rs", "src/ui.rs"] {
        let text = fs::read_to_string(fixture.join(file)).unwrap();
        let text = text
            .replace("mod theme;\n", "")
            .replace("use crate::theme::format_count;\n", "use rawr::preview;\n")
            .replace("format_count(counter.value)", "counter.value")
            .replace("    fn update(", "    pub fn update(")
            .replace(
                "[dependencies]\n",
                "[dependencies]\nrawr-macros = { path = \"../macros\" }\n",
            )
            .replace(
                "pub fn view(",
                "#[preview]\n#[rawr::preview(name = \"seven\", seed = seven, size = (200, 160))]\npub fn view(",
            );
        fs::write(root.join(file), text).unwrap();
    }
    let ui = root.join("src/ui.rs");
    let mut text = fs::read_to_string(&ui).unwrap();
    text.push_str("\nfn seven() -> Counter {\n    Counter { value: 7 }\n}\n");
    fs::write(&ui, text).unwrap();

    let loaded = rawr_cli::base::load(Some(&root)).unwrap();
    assert!(!loaded.graph.declares_rust_import());
    let names = loaded
        .graph
        .previews()
        .into_iter()
        .map(|preview| preview.name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["view", "seven"]);
    drop(loaded);

    let prepared = rawr_iced_cli::run::prepare(Some(root)).unwrap();
    assert!(prepared.adapter().diagnostics().is_empty());
    assert!(prepared.adapter().last_good_output().is_some());
}

/// A copy of the fixture directory `name` from `cli/tests/fixtures` at
/// `into`, without build products.
fn copy_fixture(name: &str, into: &Path) -> PathBuf {
    fn visit(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if name == "target" || name == "Cargo.lock" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                visit(&path, &to.join(&name));
            } else {
                fs::copy(&path, to.join(&name)).unwrap();
            }
        }
    }
    let root = into.join(name);
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../cli/tests/fixtures")
            .join(name),
        &root,
    );
    root
}

/// A marked crate that is no iced app runs with a Rust path dependency.
/// The import adds iced at rawr-iced's revision for the profile, since the
/// crate declares none.
#[test]
#[ignore = "builds the path dependency and a runner with the import driver"]
fn a_plain_marked_crate_runs_with_a_path_dependency() {
    let sandbox = Sandbox::new("path-dependency");
    let root = copy_fixture("rs-marker-path", sandbox.path());

    let output = rawr_command()
        .arg("run")
        .arg(root.join("app"))
        .output()
        .unwrap();

    writable(&root);
    assert_success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "double: 42\n");
}

/// Let the sandbox remove `root`: the import store keeps its files
/// read-only.
fn writable(root: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = fs::symlink_metadata(root).unwrap();
    if metadata.file_type().is_symlink() {
        return;
    }
    let mode = metadata.permissions().mode();
    fs::set_permissions(root, fs::Permissions::from_mode(mode | 0o200)).unwrap();
    if metadata.is_dir() {
        for entry in fs::read_dir(root).unwrap() {
            writable(&entry.unwrap().path());
        }
    }
}

/// A struct deriving `Default` with a field of an imported Rust type: the
/// field's default is `<Gauge as Default>::default()` through the bridge,
/// as `Gauge::default()` and `Default::default()` are.
#[test]
#[ignore = "builds the path dependency and a runner with the import driver"]
fn a_derived_default_calls_an_imported_type_default() {
    let sandbox = Sandbox::new("imported-default");
    let root = copy_fixture("rs-marker-path", sandbox.path());
    fs::write(
        root.join("app/src/main.rs"),
        r#"// rawr
use gauge::Gauge;

#[derive(Default)]
struct Panel {
    gauge: Gauge,
    count: u32,
}

fn main() {
    let panel = Panel::default();
    let other: Panel = Default::default();
    let gauge: Gauge = Default::default();
    println!("{} {} {} {}", panel.gauge.level(), other.gauge.level(), gauge.level(), panel.count);
}
"#,
    )
    .unwrap();

    let output = rawr_command()
        .arg("run")
        .arg(root.join("app"))
        .output()
        .unwrap();

    writable(&root);
    assert_success(&output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7 7 7 0\n");
}

/// A struct deriving `Debug` with a field of an imported Rust type prints
/// the field with its crate's `Debug`, as Rust does.
#[test]
#[ignore = "builds the path dependency and a runner with the import driver"]
fn a_derived_debug_prints_an_imported_field() {
    let sandbox = Sandbox::new("imported-debug");
    let root = copy_fixture("rs-marker-path", sandbox.path());
    fs::write(
        root.join("app/src/main.rs"),
        r#"// rawr
use gauge::Gauge;

#[derive(Debug)]
enum Reading {
    Level(Gauge),
    Missing,
}

#[derive(Debug)]
struct Panel {
    gauge: Gauge,
    reading: Reading,
}

fn main() {
    let panel = Panel { gauge: Gauge::default(), reading: Reading::Level(Gauge::default()) };
    println!("{panel:?}");
    println!("{:?} {:?}", Gauge::default(), Reading::Missing);
}
"#,
    )
    .unwrap();

    let output = rawr_command()
        .arg("run")
        .arg(root.join("app"))
        .output()
        .unwrap();

    writable(&root);
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Panel { gauge: Gauge { level: 7 }, reading: Level(Gauge { level: 7 }) }\nGauge { level: 7 } Missing\n"
    );
}

/// A script enum deriving `Clone` over an imported `Clone` type copies the
/// value: each copy can be moved into a Rust function.
#[test]
#[ignore = "builds the path dependency and a runner with the import driver"]
fn a_derived_clone_copies_an_imported_field() {
    let sandbox = Sandbox::new("imported-clone");
    let root = copy_fixture("rs-marker-path", sandbox.path());
    let library = root.join("gauge/src/lib.rs");
    let mut gauge = fs::read_to_string(&library).unwrap();
    gauge.push_str(
        r#"
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
}

pub fn shout(label: Label) -> String {
    label.text.to_uppercase()
}
"#,
    );
    fs::write(&library, gauge).unwrap();
    fs::write(
        root.join("app/src/main.rs"),
        r#"// rawr
use gauge::{shout, Label};

#[derive(Debug, Clone)]
enum Message {
    Named(Label),
}

fn unwrap(message: Message) -> Label {
    match message {
        Message::Named(label) => label,
    }
}

fn main() {
    let first = Message::Named(Label { text: String::from("hi") });
    let second = first.clone();
    let label = unwrap(first);
    let again = unwrap(second);
    println!("{} {} {again:?}", shout(label), shout(again.clone()));
}
"#,
    )
    .unwrap();

    let output = rawr_command()
        .arg("run")
        .arg(root.join("app"))
        .output()
        .unwrap();

    writable(&root);
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "HI HI Label { text: \"hi\" }\n"
    );
}

/// A generic imported type whose crate derives Copy, as chrono's
/// DateTime<Tz> is, lets a script enum derive Copy over it.
#[test]
#[ignore = "builds the path dependency and a runner with the import driver"]
fn a_derived_copy_takes_a_generic_imported_field() {
    let sandbox = Sandbox::new("imported-generic-copy");
    let root = copy_fixture("rs-marker-path", sandbox.path());
    let library = root.join("gauge/src/lib.rs");
    let mut gauge = fs::read_to_string(&library).unwrap();
    gauge.push_str(
        r#"
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading<T> {
    pub value: T,
}

pub fn reading() -> Reading<i32> {
    Reading { value: 3 }
}
"#,
    );
    fs::write(&library, gauge).unwrap();
    fs::write(
        root.join("app/src/main.rs"),
        r#"// rawr
use gauge::{reading, Reading};

#[derive(Debug, Clone, Copy)]
enum Message {
    Read(Reading<i32>),
}

fn main() {
    let first = Message::Read(reading());
    let second = first;
    println!("{first:?} {second:?}");
}
"#,
    )
    .unwrap();

    let output = rawr_command()
        .arg("run")
        .arg(root.join("app"))
        .output()
        .unwrap();

    writable(&root);
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Read(Reading { value: 3 }) Read(Reading { value: 3 })\n"
    );
}
