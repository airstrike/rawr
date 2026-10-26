//! iced's pane_grid example keeps its panes in a generic `State<Pane>` it
//! reaches through `use iced::widget::pane_grid;`. A marked crate does the
//! same with a small copy of that API and prints what `cargo run` prints:
//! types and struct literals through the module, `Option` of a tuple, a
//! `&mut self` method on a field of `self`, struct patterns of imported
//! structs and struct variants, and `get` and `get_mut`, whose `Option<&T>`
//! and `Option<&mut T>` a pattern reads and writes back. A method's lent
//! `&str` is a `&str` borrowed for the call (D568). A type alias of a
//! builtin, as iced's `markdown::Uri` is `String`, names the builtin
//! through its module. Build the driver
//! first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::copy;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/panes");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_generic_state_of_script_panes_reads_and_writes_as_cargo_run_does() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("panes");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    let expected = text(&native.stdout);
    assert!(
        expected.contains("Some(Pane { id: 1, is_pinned: true })")
            && expected.starts_with("TOP!\nmid\nleft true\n5\n"),
        "{expected}"
    );

    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        expected,
        "stderr:\n{}",
        text(&run.stderr)
    );
}
