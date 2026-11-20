//! `rawr-play` on a snippet whose frontmatter names an approved crate
//! (`play/src/script.rs`): the snippet's dependency is built through
//! rawr-import's script path, offline, and the snippet runs on the runner
//! built for it, in a fork of that runner's server, as a snippet runs on
//! the prebuilt runner.
//!
//! They need rawr-import built from this checkout, with rawr's runtime and
//! its driver for this checkout, which the first build makes. So they are
//! ignored by default:
//!
//! ```sh
//! cargo build -p rawr-import-cli --bin rawr-import
//! cargo test -p rawr-play --test script -- --ignored
//! ```
//!
//! `PLAY_IMPORT` names another rawr-import. The build is offline, so the
//! crates the snippet names must be in Cargo's registry cache here.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The repository's root.
fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Run `source` through the CLI with the approved crates of
/// `play/approved`, and give its exit status, stdout and stderr.
fn run(source: &str) -> std::process::Output {
    use std::io::Write as _;
    let import = std::env::var_os("PLAY_IMPORT").map_or_else(
        || repository().join("target/debug/rawr-import"),
        PathBuf::from,
    );
    assert!(
        import.is_file(),
        "needs rawr-import: cargo build -p rawr-import-cli --bin rawr-import"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .args(["-", "--time-ms", "60000", "-o", "-"])
        .env(
            rawr_play::approved::DIRECTORY,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("approved"),
        )
        .env(rawr_play::script::IMPORT, &import)
        .env(
            rawr_play::script::SCRIPTS,
            repository().join("target/play-scripts"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// ratatui, an approved crate, is built for the snippet, and its `Rect`
/// gives what `cargo run` of the same script prints.
#[test]
#[ignore = "needs rawr-import and rawr's runtime for this checkout"]
fn a_frontmatter_snippet_runs_with_its_approved_dependency() {
    let output = run(concat!(
        "---\n",
        "[dependencies]\n",
        "ratatui = { version = \"0.30\", default-features = false }\n",
        "---\n",
        "use ratatui::layout::Rect;\n",
        "\n",
        "fn main() {\n",
        "    println!(\"{}\", Rect::new(0, 0, 20, 4).area());\n",
        "}\n",
    ));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let ran = rawr_play::Output::from_json(&output.stdout).unwrap();
    assert_eq!((ran.text().as_str(), ran.status), ("80\n", 0), "{stderr}");
}
