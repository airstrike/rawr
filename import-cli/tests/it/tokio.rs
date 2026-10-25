//! A marked `#[tokio::main] async fn main` run by the real `rawr-import`
//! binary on the project's own tokio (D513): the shadow crate's entry carries
//! the attribute as written, script futures are std futures, a channel
//! carries a script struct, and a save restarts the program. Build the
//! driver first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::support::{copy, start};

fn fixture(name: &str) -> PathBuf {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &root,
    );
    root
}

/// `job N at T ms: ..`: `N` and `T`.
fn job(line: &str) -> Option<(u64, u64)> {
    let rest = line.strip_prefix("job ")?;
    let (id, rest) = rest.split_once(" at ")?;
    let (millis, _) = rest.split_once(" ms")?;
    Some((id.parse().ok()?, millis.parse().ok()?))
}

#[test]
fn an_async_main_runs_on_the_projects_tokio_with_a_channel_join_and_spawn_blocking() {
    let root = fixture("tokio-async");
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 4, "{stdout}");
    // Each job arrives after its producer's real 100 ms sleeps, in order,
    // carrying the script struct its `Debug` prints.
    for (index, line) in lines[..3].iter().enumerate() {
        let id = index as u64 + 1;
        let (got, millis) = job(line).unwrap_or_else(|| panic!("{stdout}"));
        assert_eq!(got, id, "{stdout}");
        assert!(millis >= id * 100, "{stdout}");
        assert!(
            line.ends_with(&format!("Job {{ id: {id}, name: \"job {id}\" }}")),
            "{stdout}"
        );
    }
    assert_eq!(lines[3], "total 6, 303 primes");
    // The runner links the project's tokio, with no iced.
    let runner = fs::read_to_string(root.join("target/rawr/runner/Cargo.toml")).unwrap();
    assert!(runner.contains("tokio = "), "{runner}");
    assert!(!runner.contains("iced"), "{runner}");
    // The shadow crate's entry carries the attribute as written.
    let entry = fs::read_to_string(root.join("target/rawr/crate/lib.rs")).unwrap();
    assert!(
        entry.contains("    #[tokio::main]\n    async fn main() -> i32 {"),
        "{entry}"
    );
}

#[test]
fn a_save_restarts_an_async_main_on_its_runtime_as_its_attribute_says() {
    let root = fixture("tokio-ticks");
    let main = root.join("src/main.rs");
    let mut run = start(&root, &[]);

    // The first run builds the runner, on a current-thread runtime.
    let first = run.until(Duration::from_secs(900), |line| line == "version 1 tick 3");
    assert_eq!(first, "version 1 tick 3");
    let entry = fs::read_to_string(root.join("target/rawr/crate/lib.rs")).unwrap();
    assert!(
        entry.contains("#[tokio::main(flavor = \"current_thread\")]"),
        "{entry}"
    );

    let text = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        text.replace("const VERSION: i64 = 1;", "const VERSION: i64 = 2;"),
    )
    .unwrap();
    let restarted = run.until(Duration::from_secs(600), |line| {
        line.starts_with("version 2")
    });
    assert_eq!(restarted, "version 2 tick 1");
    assert!(
        run.seen
            .iter()
            .any(|line| line == "stderr: rawr: the source changed; restarting the program"),
        "{}",
        run.seen.join("\n")
    );
}
