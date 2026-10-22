//! Cargo crates of `.rs` files marked `// rawr`: discovery through the
//! crate's `mod` tree, the parse cells of a reload, and `cargo check`
//! beside Rawr.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use super::{assert_success, stderr, stdout, Sandbox};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// A copy of the fixture crate `name` at `into`, without build products.
fn copy(name: &str, into: &Path) -> PathBuf {
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
    visit(&Path::new(FIXTURES).join(name), &root);
    root
}

fn graph(
    root: &Path,
    cells: Option<&rawr::package::Cells>,
) -> Result<rawr::package::Graph, String> {
    let target = rawr::project::Target::discover(root).map_err(|error| error.to_string())?;
    let engine = rawr::Engine::new();
    match cells {
        Some(cells) => rawr::package::Graph::load_with_cells(&engine, &target, cells),
        None => rawr::package::Graph::load(&engine, &target),
    }
    .map_err(|error| error.to_string())
}

#[test]
fn a_marked_crate_runs_and_checks_like_a_rawr_project() {
    let sandbox = Sandbox::new("marked-cart");
    let root = copy("rs-marker-cart", &sandbox.work());

    let run = sandbox.rawr(&["run", "rs-marker-cart"]);
    let file = sandbox.rawr(&["run", "rs-marker-cart/src/main.rs"]);
    let checked = sandbox.rawr(&["check", "rs-marker-cart"]);

    assert_success(&run);
    assert_eq!(stdout(&run), "total: 27\n");
    assert_eq!(
        stderr(&run),
        "cargo is not on PATH. Rawr skipped `cargo check`.\n"
    );
    assert_success(&file);
    assert_eq!(stdout(&file), "total: 27\n");
    assert_success(&checked);
    assert_eq!(
        stdout(&checked),
        format!("Checked {}\n", root.join("src/main.rs").display())
    );
    assert_eq!(
        stderr(&checked),
        "cargo is not on PATH. Rawr skipped `cargo check`.\n"
    );
}

#[test]
fn cargo_rawr_forwards_to_rawr() {
    let sandbox = Sandbox::new("cargo-rawr");
    let root = copy("rs-marker-cart", &sandbox.work());
    fs::copy(
        env!("CARGO_BIN_EXE_cargo-rawr"),
        sandbox.path.join("bin/cargo-rawr"),
    )
    .unwrap();

    // Cargo runs `cargo rawr run <path>` as `cargo-rawr rawr run <path>`.
    let output = Command::new(sandbox.path.join("bin/cargo-rawr"))
        .args(["rawr", "run"])
        .arg(&root)
        .env("PATH", "")
        .env_remove("CARGO")
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(stdout(&output), "total: 27\n");
}

#[test]
fn discovery_follows_mod_declarations_and_ignores_other_files() {
    let sandbox = Sandbox::new("marked-tree");
    let root = copy("rs-marker-cart", &sandbox.work());
    // Not valid Rust or Rawr: loading it would fail.
    assert!(root.join("src/unused.rs").is_file());

    let graph = graph(&root, None).unwrap();

    let sources: Vec<_> = graph
        .sources()
        .iter()
        .map(|source| source.path().strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        sources,
        [PathBuf::from("src/main.rs"), PathBuf::from("src/item.rs")]
    );
    assert!(graph.entry_package().is_marked());
    assert!(!graph.declares_rust_import());

    // `mod item;` also finds `item/mod.rs`, as rustc does.
    fs::create_dir(root.join("src/item")).unwrap();
    fs::rename(root.join("src/item.rs"), root.join("src/item/mod.rs")).unwrap();
    let graph = self::graph(&root, None).unwrap();
    assert!(graph.sources()[1].path().ends_with("src/item/mod.rs"));
}

#[test]
fn a_reachable_unmarked_module_is_a_clear_error() {
    let sandbox = Sandbox::new("marked-native");
    let root = copy("rs-marker-cart", &sandbox.work());
    let item = root.join("src/item.rs");
    let text = fs::read_to_string(&item).unwrap();
    fs::write(&item, text.trim_start_matches("// rawr\n")).unwrap();

    let checked = sandbox.rawr(&["check", "rs-marker-cart"]);

    assert_eq!(checked.status.code(), Some(1));
    let text = stderr(&checked);
    assert!(
        text.contains(&format!(
            "module `item` is native Rust. `{}` does not start with `// rawr`. \
             Rawr does not run native modules yet. Add `// rawr` as its first line to run it with Rawr.",
            item.display()
        )),
        "{text}"
    );
    assert!(text.contains("mod item;"), "{text}");
}

#[test]
fn an_unmarked_crate_root_is_not_a_rawr_program() {
    let sandbox = Sandbox::new("marked-root");
    let root = copy("rs-marker-cart", &sandbox.work());
    let main = root.join("src/main.rs");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.replacen("// rawr\n", "//rawr\n", 1)).unwrap();

    let checked = sandbox.rawr(&["check", "rs-marker-cart"]);

    assert_eq!(checked.status.code(), Some(1));
    let text = stderr(&checked);
    assert!(
        text.contains(&format!(
            "`{}` is not a Rawr file. Its first line is not `// rawr`.",
            main.display()
        )),
        "{text}"
    );
    assert!(text.contains("add `// rawr` as the first line"), "{text}");
}

/// A live session loads again on each save through remembering cells:
/// only the saved file is parsed again.
#[test]
fn a_reload_parses_only_the_file_that_changed() {
    let sandbox = Sandbox::new("marked-cells");
    let root = copy("rs-marker-cart", &sandbox.work());
    let cells = rawr::package::Cells::new(sandbox.path.join("cells")).remember();

    graph(&root, Some(&cells)).unwrap();
    assert_eq!((cells.counts().parsed, cells.counts().kept), (2, 0));

    // Unchanged: nothing is parsed.
    graph(&root, Some(&cells)).unwrap();
    assert_eq!((cells.counts().parsed, cells.counts().kept), (2, 2));

    let item = root.join("src/item.rs");
    let text = fs::read_to_string(&item).unwrap();
    fs::write(&item, text.replace("3.5", "4.0")).unwrap();
    let graph = graph(&root, Some(&cells)).unwrap();
    assert_eq!(cells.counts().parsed, 3);
    assert!(graph.sources()[1].text().contains("4.0"));
}

/// `rawr check` runs the real `cargo check`: rustc rejects what Rawr
/// accepts, and its error comes after Rawr's result, under its heading.
#[test]
fn rawr_check_reports_what_only_rustc_rejects() {
    let Some(cargo) = std::env::var_os("CARGO") else {
        panic!("run under cargo: $CARGO names the cargo to test with");
    };
    let sandbox = Sandbox::new("marked-rustc");
    let root = copy("rs-marker-cart", &sandbox.work());
    let main = root.join("src/main.rs");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        text.replace(
            "    println!(\"total: {}\", total(&cart));",
            "    let label: String = \"total\";\n    println!(\"{label}: {}\", total(&cart));",
        ),
    )
    .unwrap();
    let bin = Path::new(&cargo).parent().unwrap();

    let output = Command::new(sandbox.path.join("bin/rawr"))
        .args(["check", "rs-marker-cart"])
        .current_dir(sandbox.work())
        .env("PATH", bin)
        .env("CARGO", &cargo)
        .env("CARGO_TARGET_DIR", sandbox.path.join("target"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("Checked {}\n", main.display()));
    let text = stderr(&output);
    assert!(
        text.starts_with("\ncargo check found 1 error:\n\nerror[E0308]: mismatched types"),
        "{text}"
    );
    assert!(text.contains("src/main.rs:16"), "{text}");
}

/// The background check of `rawr run`, with a stand-in cargo that takes
/// a second: a save stops the running check, saves close together start
/// one new check, and only that check is reported.
#[cfg(unix)]
#[test]
fn background_cargo_check_restarts_on_saves_and_reports_only_the_newest() {
    use std::os::unix::fs::PermissionsExt as _;

    let sandbox = Sandbox::new("marked-background");
    let log = sandbox.path.join("log");
    let cargo = sandbox.path.join("bin/cargo");
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\n\
             echo started >> '{log}/started'\n\
             n=$(wc -l < '{log}/started' | tr -d ' ')\n\
             sleep 1\n\
             echo \"$n\" >> '{log}/finished'\n\
             printf '{{\"reason\":\"compiler-message\",\"message\":{{\"level\":\"error\",\"message\":\"run %s\",\"rendered\":\"error: run %s\"}}}}\\n' \"$n\" \"$n\"\n\
             exit 101\n",
            log = log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir(&log).unwrap();
    let lines = |name: &str| {
        fs::read_to_string(log.join(name))
            .map(|text| text.lines().map(str::to_owned).collect::<Vec<_>>())
            .unwrap_or_default()
    };

    let (reported, reports) = mpsc::channel();
    let background = rawr_cli::cargo::Background::start_with(
        cargo.into_os_string(),
        sandbox.path.join("Cargo.toml"),
        Duration::from_millis(200),
        move |status| drop(reported.send(status)),
    );
    // The launch check starts at once. Three saves stop it and start one.
    let launched = std::time::Instant::now();
    while lines("started").is_empty() {
        if let Ok(status) = reports.try_recv() {
            panic!("the stand-in cargo did not start: {status:?}");
        }
        assert!(
            launched.elapsed() < Duration::from_secs(10),
            "no check started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..3 {
        background.saved();
        std::thread::sleep(Duration::from_millis(20));
    }

    let status = reports.recv_timeout(Duration::from_secs(10)).unwrap();
    let rawr_cli::cargo::Status::Finished(outcome) = status else {
        panic!("{status:?}");
    };
    assert_eq!(outcome.errors, ["error: run 2"]);
    assert!(reports.recv_timeout(Duration::from_millis(1500)).is_err());
    drop(background);
    assert_eq!(lines("started").len(), 2);
    assert_eq!(lines("finished"), ["2"]);
}
