//! A script's own types and functions cross into a native generic entry
//! point in the shape of a GUI library's `run` (D594): `run::<S, M, U, V>`
//! makes the state with the script type's `Default`, names it by
//! `type_name`, and calls the script's `update(&mut S, M)` and its
//! higher-ranked `view(&S) -> Widget<'_, M>`, whose label borrows the
//! state's string. The output is `cargo run`'s, and a body edit takes
//! effect at the next event with the state kept. Build the driver first,
//! as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::support::{copy, start};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crossing");

/// The fixture copied to its own root `name` under the test directory.
fn root(name: &str) -> PathBuf {
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
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    root
}

#[test]
fn a_script_state_message_update_and_view_run_under_a_native_generic_run() {
    let root = root("crossing");
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .env_remove("EVENTS_COUNT")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    // `cargo run` of the same crate prints this, but for the process id.
    let printed = stdout
        .lines()
        .filter(|line| !line.starts_with("pid "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        printed,
        "title crossing::Counter\n1 view \n1 message Increment\n2 view count 1\n\
         2 message Increment\n3 view count 2\n3 message Increment\nlast view count 3",
        "stderr:\n{stderr}"
    );
}

/// `event` and the count a `N view count X` line shows.
fn parse(line: &str) -> Option<(i64, i64)> {
    let (event, count) = line.split_once(" view count ")?;
    Some((event.trim().parse().ok()?, count.trim().parse().ok()?))
}

#[test]
fn a_body_edit_takes_effect_at_the_next_event_with_the_state_kept() {
    let root = root("crossing-swap");
    let main = root.join("src/main.rs");
    let mut run = start(
        &root,
        &[
            ("EVENTS_COUNT", PathBuf::from("3000")),
            ("EVENTS_STOP", root.join("stop")),
        ],
    );
    let pid = run.until(Duration::from_secs(900), |line| line.starts_with("pid "));
    let before = run.until(Duration::from_secs(60), |line| {
        parse(line).is_some_and(|(_, count)| count >= 3)
    });
    let (_, kept) = parse(&before).unwrap();

    let text = fs::read_to_string(&main).unwrap();
    assert!(text.contains("counter.value += 1;"), "{text}");
    fs::write(
        &main,
        text.replace("counter.value += 1;", "counter.value += 10;"),
    )
    .unwrap();
    // The next events count by ten from where the state was.
    let previous = std::cell::Cell::new(None::<i64>);
    run.until(Duration::from_secs(120), |line| {
        let Some((_, count)) = parse(line) else {
            return false;
        };
        let stepped = previous
            .get()
            .is_some_and(|previous| count - previous == 10);
        previous.set(Some(count));
        stepped
    });
    let counts = run
        .seen
        .iter()
        .filter_map(|line| parse(line))
        .map(|(_, count)| count)
        .collect::<Vec<_>>();
    let jump = counts
        .windows(2)
        .position(|pair| pair[1] - pair[0] == 10)
        .unwrap_or_else(|| panic!("{}", run.seen.join("\n")));
    // The state was kept: the count went on from where it was.
    assert!(counts[jump] >= kept, "{}", run.seen.join("\n"));
    assert!(
        counts[..=jump]
            .windows(2)
            .all(|pair| pair[1] - pair[0] == 1),
        "{}",
        run.seen.join("\n")
    );
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
    assert_eq!(
        run.seen
            .iter()
            .filter(|line| line.starts_with("pid "))
            .count(),
        1,
        "{seen}"
    );
    assert!(run.seen.contains(&pid));

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}
