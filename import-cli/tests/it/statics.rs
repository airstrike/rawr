//! Statics declared inside marked bodies, of native types (D862): two
//! written in one block that refer to each other, and tracing's event shape,
//! which a crate's macro expands at each call to a `__CALLSITE` static whose
//! initializer declares a `META` static pointing back at it, built with
//! const fns and registered by address through `interest(&'static self)`.
//! Each is one native place for the program's life: a save that leaves its
//! declaration alone keeps it, and one that changes it makes a new one
//! without a restart. Build the driver first, as for `islands`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::{copy, start};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/marked-statics");

fn fixture(name: &str) -> PathBuf {
    let root = crate::support::scratch().join(name);
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    root
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{text}");
    fs::write(path, text.replace(from, to)).unwrap();
}

fn rawr_import(command: &str, root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg(command)
        .arg(root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env("PROBE_COUNT", "3")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

#[test]
fn statics_declared_in_bodies_check_and_run_as_cargo_runs_them() {
    let root = fixture("marked-statics");
    let check = rawr_import("check", &root);
    assert!(
        check.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&check.stdout),
        text(&check.stderr)
    );

    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env("PROBE_COUNT", "3")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    assert_eq!(
        text(&native.stdout),
        "written registered 1 of 1 back true\n\
         written registered 1 of 1 back true\n\
         first registered 2 of 2 back true\n\
         1 value 1 tick registered 3 of 3 back true\n\
         2 value 2 tick registered 3 of 3 back true\n\
         3 value 3 tick registered 3 of 3 back true\n"
    );

    let run = rawr_import("run", &root);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), text(&native.stdout));
}

#[test]
fn a_body_static_keeps_its_place_across_a_save_and_moves_when_its_declaration_changes() {
    let root = fixture("marked-statics-swap");
    let main = root.join("src/main.rs");
    let mut run = start(&root, &[("PROBE_STOP", root.join("stop"))]);
    run.until(|line| line.starts_with("2 value 2 "));
    assert!(
        run.seen
            .iter()
            .any(|line| line == "1 value 1 tick registered 3 of 3 back true"),
        "{}",
        run.seen.join("\n")
    );

    // A body edit beside the event: the new generation's event is the same
    // callsite, registered once, at the same address.
    edit(&main, "let scale = 1;", "let scale = 100;");
    let swapped = run.until(|line| line.contains("00 tick registered"));
    assert!(
        swapped.ends_with(" tick registered 3 of 3 back true"),
        "{}",
        run.seen.join("\n")
    );

    // A changed initializer: a new callsite, which registers at its own
    // address, in the same process.
    edit(&main, "probe::event!(\"tick\")", "probe::event!(\"tock\")");
    let moved = run.until(|line| line.contains(" tock "));
    assert!(
        moved.ends_with(" tock registered 4 of 4 back true"),
        "{}",
        run.seen.join("\n")
    );
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
    assert_eq!(
        run.seen
            .iter()
            .filter(|line| line.starts_with("written registered"))
            .count(),
        2,
        "{seen}"
    );

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}
