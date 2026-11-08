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
    assert!(!graph.declares_rust_import());

    // `mod item;` also finds `item/mod.rs`, as rustc does.
    fs::create_dir(root.join("src/item")).unwrap();
    fs::rename(root.join("src/item.rs"), root.join("src/item/mod.rs")).unwrap();
    let graph = self::graph(&root, None).unwrap();
    assert!(graph.sources()[1].path().ends_with("src/item/mod.rs"));
}

/// An unmarked module of a marked crate is native Rust: Rawr records it for
/// the native build and parses only the marked files. A crate with a native
/// module imports its Cargo dependencies.
#[test]
fn an_unmarked_module_is_native_rust() {
    let sandbox = Sandbox::new("marked-native");
    let root = copy("rs-marker-native", &sandbox.work());

    let graph = graph(&root, None).unwrap();

    let sources: Vec<_> = graph
        .sources()
        .iter()
        .map(|source| source.path().strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        sources,
        [
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/receipt.rs")
        ]
    );
    let native = graph.entry_package().rust_modules();
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].modules(), ["tax"]);
    assert_eq!(native[0].path(), root.join("src/tax.rs"));
    assert!(graph.declares_rust_import());
}

/// A marked iced crate imports its Cargo dependencies with or without a
/// native module: it runs against the iced its own `cargo build` builds.
#[test]
fn a_marked_iced_crate_imports_its_dependencies_with_or_without_a_native_module() {
    let sandbox = Sandbox::new("marked-counter");
    let root = copy("rs-marker-counter", &sandbox.work());

    let graph = self::graph(&root, None).unwrap();
    let native = graph.entry_package().rust_modules();
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].modules(), ["theme"]);
    assert!(graph.declares_rust_import());

    fs::remove_file(root.join("src/theme.rs")).unwrap();
    let main = root.join("src/main.rs");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.replace("mod theme;\n", "")).unwrap();
    let ui = root.join("src/ui.rs");
    let text = fs::read_to_string(&ui).unwrap();
    fs::write(
        &ui,
        text.replace("use crate::theme::format_count;\n", "")
            .replace("format_count(counter.value)", "counter.value"),
    )
    .unwrap();
    let graph = self::graph(&root, None).unwrap();
    assert!(graph.entry_package().rust_modules().is_empty());
    assert!(graph.declares_rust_import());
}

/// A crate whose root is native Rust and declares a marked module is a
/// Rawr project (D501). Rawr reads the root's `mod` declarations, as the
/// root of the marked modules, and the marked files; the root and the
/// other modules are native, and the crate imports Rust.
#[test]
fn an_unmarked_root_that_declares_a_marked_module_is_a_project() {
    let sandbox = Sandbox::new("marked-islands");
    let root = copy("rs-marker-islands", &sandbox.work());

    let graph = graph(&root, None).unwrap();

    let sources: Vec<_> = graph
        .sources()
        .iter()
        .map(|source| source.path().strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        sources,
        [
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/physics.rs")
        ]
    );
    // The root's Rawr text: its declarations at their places, and a `main`
    // that starts the native one.
    let rawr = graph.sources()[0].text();
    assert_eq!(rawr.lines().nth(1), Some("mod physics;"));
    assert_eq!(rawr.lines().nth(2), Some("mod units;"));
    assert!(!rawr.contains("ball.advance"), "{rawr}");
    let package = graph.entry_package();
    assert_eq!(
        package.native_root().map(rawr::package::RustModule::path),
        Some(root.join("src/main.rs").as_path())
    );
    assert_eq!(package.rust_modules().len(), 1);
    assert_eq!(package.rust_modules()[0].modules(), ["units"]);
    assert!(graph.declares_rust_import());
    assert!(graph.island_shape().is_some_and(|shape| !shape.is_empty()));

    // Without a marked module, the unmarked root is no Rawr program.
    let physics = root.join("src/physics.rs");
    let text = fs::read_to_string(&physics).unwrap();
    fs::write(&physics, text.replacen("// rawr\n", "", 1)).unwrap();
    let error = self::graph(&root, None).unwrap_err();
    assert!(
        error.contains("no module it declares starts with it"),
        "{error}"
    );
}

/// With a marked crate root, native code must not use marked code: Rawr
/// builds the native modules without the marked ones. Cargo builds the
/// same crate. (A native root calls its marked modules through rawr.)
#[test]
fn native_code_cannot_use_marked_code() {
    let sandbox = Sandbox::new("marked-direction");
    let root = copy("rs-marker-direction", &sandbox.work());
    let pricing = root.join("src/pricing.rs");

    let error = graph(&root, None).unwrap_err();

    assert!(
        error.contains(&format!(
            "native module `crate::pricing` cannot use `crate::Item`, which is in the marked \
             module `crate` (`{}`): native modules cannot use marked code when the crate root \
             is marked. \
             Move the item into a native module, mark `{}` with `// rawr`, \
             or leave the crate root unmarked so native code calls marked modules through rawr.",
            root.join("src/main.rs").display(),
            pricing.display()
        )),
        "{error}"
    );

    // A native module may use another native module, through `crate` or
    // `super`, and a test module's `super` is its own module.
    fs::write(
        root.join("src/money.rs"),
        "pub fn twice(cents: i64) -> i64 { cents * 2 }\n",
    )
    .unwrap();
    fs::write(
        &pricing,
        "use crate::money::twice;\n\
         pub fn doubled(cents: i64) -> i64 { super::money::twice(cents) + twice(0) }\n\
         #[cfg(test)]\nmod tests { use super::doubled; }\n",
    )
    .unwrap();
    let main = root.join("src/main.rs");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(
        &main,
        text.replace("mod pricing;", "mod money;\nmod pricing;"),
    )
    .unwrap();
    graph(&root, None).unwrap();

    // A marked module is marked code too, reached through `super`.
    fs::write(
        root.join("src/money.rs"),
        "// rawr\npub fn twice(cents: i64) -> i64 { cents * 2 }\n",
    )
    .unwrap();
    let error = graph(&root, None).unwrap_err();
    assert!(
        error.contains("native module `crate::pricing` cannot use `crate::money::twice`")
            && error.contains("is in the marked module `crate::money`"),
        "{error}"
    );
}

/// A package with a library and a binary whose marked modules are all in
/// the library's tree runs from its library (#1107). Rawr reads the
/// library root's `mod` declarations and the marked files; the binary is
/// native, its own crate with its own modules, and it is the program's
/// `main`. It reaches the marked module through the library, as Cargo
/// builds it, and so does a native module of the library.
#[test]
fn a_marked_module_of_the_library_runs_from_the_native_binary() {
    let sandbox = Sandbox::new("marked-lib");
    let root = copy("rs-marker-lib", &sandbox.work());

    let graph = graph(&root, None).unwrap();

    let sources: Vec<_> = graph
        .sources()
        .iter()
        .map(|source| source.path().strip_prefix(&root).unwrap().to_path_buf())
        .collect();
    assert_eq!(
        sources,
        [PathBuf::from("src/lib.rs"), PathBuf::from("src/score.rs")]
    );
    let package = graph.entry_package();
    assert_eq!(package.name(), "rs_marker_lib");
    assert_eq!(
        package.native_root().map(rawr::package::RustModule::path),
        Some(root.join("src/lib.rs").as_path())
    );
    let native = package
        .rust_modules()
        .iter()
        .map(|module| module.modules().join("::"))
        .collect::<Vec<_>>();
    assert_eq!(native, ["report", "units"]);
    let bin = package.bin().expect("the binary runs the program");
    assert_eq!(bin.root().path(), root.join("src/main.rs"));
    let modules = bin
        .modules()
        .iter()
        .map(|module| (module.modules().join("::"), module.path().to_path_buf()))
        .collect::<Vec<_>>();
    assert_eq!(modules, [("rounds".to_owned(), root.join("src/rounds.rs"))]);
    // The library's Rawr text starts the binary's `main`.
    let rawr = graph.entry_source().text();
    assert!(
        rawr.ends_with("fn main() {\n    __rawr_bin::__rawr_island::main();\n}\n"),
        "{rawr}"
    );
    assert!(graph.declares_rust_import());
    let shape = graph.island_shape().unwrap();
    assert_eq!(
        rawr::package::Shape::default().change(&shape),
        Some("native code can now see `score::Game`".to_owned())
    );

    // A marked library root is not run from a native binary yet.
    let library = root.join("src/lib.rs");
    let text = fs::read_to_string(&library).unwrap();
    fs::write(&library, format!("// rawr\n{text}")).unwrap();
    let error = self::graph(&root, None).unwrap_err();
    assert!(
        error.contains(&format!(
            "`{}` starts with `// rawr`, and the package's binary `{}` is native. \
             A native binary runs the marked modules of a native library root, \
             not yet a marked library root. \
             Remove `// rawr` from `{}` and mark the modules it declares.",
            library.display(),
            root.join("src/main.rs").display(),
            library.display(),
        )),
        "{error}"
    );
}

/// The library fixture builds and runs unchanged under Cargo,
/// with the marker only a comment.
#[test]
fn a_package_whose_library_is_marked_builds_under_cargo() {
    let Some(cargo) = std::env::var_os("CARGO") else {
        panic!("run under cargo: $CARGO names the cargo to test with");
    };
    let sandbox = Sandbox::new("marked-lib-cargo");
    let root = copy("rs-marker-lib", &sandbox.work());

    let output = Command::new(&cargo)
        .args(["run", "--quiet", "--offline", "--release"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", sandbox.path.join("target"))
        .env("RUSTFLAGS", "--deny warnings")
        .output()
        .unwrap();

    assert_success(&output);
    assert_eq!(stdout(&output), "score: 53 points in 3 rounds\n");
}

/// When both the binary's tree and the library's have marked modules,
/// Rawr runs neither half silently: it names both trees.
#[test]
fn marked_modules_in_both_trees_of_a_package_are_an_error() {
    let sandbox = Sandbox::new("marked-lib-both");
    let root = copy("rs-marker-lib", &sandbox.work());
    let rounds = root.join("src/rounds.rs");
    let text = fs::read_to_string(&rounds).unwrap();
    fs::write(&rounds, format!("// rawr\n{text}")).unwrap();

    let error = graph(&root, None).unwrap_err();

    assert!(
        error.contains(&format!(
            "the binary `{}` has marked modules, and so does the library `{}`: `{}` starts with `// rawr`. \
             Rawr runs the marked modules of one crate of a package, \
             the binary's when its tree has one, else the library's. \
             Remove `// rawr` from the marked files of one of the two trees.",
            root.join("src/main.rs").display(),
            root.join("src/lib.rs").display(),
            root.join("src/score.rs").display(),
        )),
        "{error}"
    );

    // With the library's marker gone, the binary's tree runs.
    let score = root.join("src/score.rs");
    let text = fs::read_to_string(&score).unwrap();
    fs::write(&score, text.replacen("// rawr\n", "", 1)).unwrap();
    let graph = self::graph(&root, None).unwrap();
    assert!(graph.entry_source().path().ends_with("src/main.rs"));
    assert!(graph.entry_package().bin().is_none());
}

/// A workspace member with a library and a binary runs from its library
/// when only the library's tree is marked, with what it inherits.
#[test]
fn a_workspace_member_runs_from_its_marked_library() {
    let sandbox = Sandbox::new("marked-workspace-lib");
    let root = copy("rs-marker-workspace", &sandbox.work());
    let dice = root.join("dice");
    let library = dice.join("src/lib.rs");
    let text = fs::read_to_string(&library).unwrap();
    fs::write(&library, format!("pub mod roll;\n{text}")).unwrap();
    fs::write(
        dice.join("src/roll.rs"),
        "// rawr\npub fn twice(seed: i64) -> i64 {\n    (seed.rem_euclid(6) + 1) * 2\n}\n",
    )
    .unwrap();
    fs::write(
        dice.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", dice::roll::twice(4));\n}\n",
    )
    .unwrap();

    let graph = graph(&dice, None).unwrap();

    assert_eq!(graph.entry_source().path(), library);
    let package = graph.entry_package();
    assert_eq!(package.name(), "dice");
    assert_eq!(
        package.bin().map(|bin| bin.root().path()),
        Some(dice.join("src/main.rs").as_path())
    );
    assert!(graph.declares_rust_import());
}

#[test]
fn an_unmarked_crate_root_is_not_a_rawr_program() {
    let sandbox = Sandbox::new("marked-root");
    let root = copy("rs-marker-cart", &sandbox.work());
    let main = root.join("src/main.rs");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.replacen("// rawr\n", "//rawr\n", 1)).unwrap();
    // No module it declares is marked either.
    let item = root.join("src/item.rs");
    let text = fs::read_to_string(&item).unwrap();
    fs::write(&item, text.replacen("// rawr\n", "", 1)).unwrap();

    let checked = sandbox.rawr(&["check", "rs-marker-cart"]);

    assert_eq!(checked.status.code(), Some(1));
    let text = stderr(&checked);
    assert!(
        text.contains(&format!(
            "`{}` is not a Rawr file. Its first line is not `// rawr`, \
             and no module it declares starts with it.",
            main.display()
        )),
        "{text}"
    );
    assert!(text.contains("add `// rawr` as the first line"), "{text}");
}

/// A cargo script is marked on its first line of Rust, after its shebang
/// and frontmatter. One with no dependencies runs on the base language,
/// and one that declares dependencies goes to rawr-import, as a crate with
/// dependencies does.
#[test]
fn a_cargo_script_runs_with_the_marker_after_its_frontmatter() {
    let sandbox = Sandbox::new("marked-script");
    let work = sandbox.work();
    fs::write(
        work.join("tool.rs"),
        "#!/usr/bin/env rawr\n---cargo\n[package]\nedition = \"2024\"\n---\n// rawr\n\
         fn main() {\n    println!(\"script\");\n}\n",
    )
    .unwrap();
    fs::write(
        work.join("deps.rs"),
        "---cargo\n[dependencies]\nrand = \"0.9\"\n---\n// rawr\nfn main() {}\n",
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "tool.rs"]);
    let handed = sandbox.rawr(&["run", "deps.rs"]);

    assert_success(&run);
    assert_eq!(stdout(&run), "script\n");
    assert_eq!(handed.status.code(), Some(1));
    let text = stderr(&handed);
    assert!(
        text.contains(
            "this program uses Rust dependencies in its frontmatter, which rawr-import serves, and it is not installed"
        ),
        "{text}"
    );
    // Nothing of either script's is kept beside it.
    assert!(!work.join("target").exists());
    assert!(!work.join("Cargo.lock").exists());
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
            "    let n = 3;\n    let shown: &'static i32 = &n;\n    println!(\"total: {} ({shown})\", total(&cart));",
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
        text.starts_with(
            "\ncargo check found 1 error:\n\nerror[E0597]: `n` does not live long enough"
        ),
        "{text}"
    );
    assert!(text.contains("src/main.rs:17:31"), "{text}");
}

/// A script gets `cargo check` too, though rustc refuses its frontmatter
/// outside `cargo -Zscript`: rustc's error names the script's own line.
#[test]
fn rawr_check_reports_what_only_rustc_rejects_in_a_script() {
    let Some(cargo) = std::env::var_os("CARGO") else {
        panic!("run under cargo: $CARGO names the cargo to test with");
    };
    let sandbox = Sandbox::new("marked-script-rustc");
    let work = fs::canonicalize(sandbox.work()).unwrap();
    let script = work.join("wide.rs");
    fs::write(
        &script,
        "#!/usr/bin/env rawr\n---cargo\n[package]\nedition = \"2024\"\n---\n// rawr\n\
         fn main() {\n    let n = 3;\n    let x: &'static i32 = &n;\n    println!(\"{x}\");\n}\n",
    )
    .unwrap();
    let bin = Path::new(&cargo).parent().unwrap();

    let output = Command::new(sandbox.path.join("bin/rawr"))
        .args(["check", "wide.rs"])
        .current_dir(&work)
        .env("PATH", bin)
        .env("CARGO", &cargo)
        .env("RAWR_HOME", sandbox.path.join("home"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    assert_eq!(stdout(&output), format!("Checked {}\n", script.display()));
    let text = stderr(&output);
    assert!(
        text.starts_with(
            "\ncargo check found 1 error:\n\nerror[E0597]: `n` does not live long enough"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!("{}:9:27", script.display())),
        "{text}"
    );
    assert_eq!(
        fs::read_dir(&work).unwrap().count(),
        1,
        "nothing beside the script"
    );
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
    let signal = sandbox.path.join("signal");
    let cargo = sandbox.path.join("bin/cargo");
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\n\
             echo started >> '{log}/started'\n\
             n=$(wc -l < '{log}/started' | tr -d ' ')\n\
             echo \"$n\" > '{signal}'\n\
             sleep 1\n\
             echo \"$n\" >> '{log}/finished'\n\
             printf '{{\"reason\":\"compiler-message\",\"message\":{{\"level\":\"error\",\"message\":\"run %s\",\"rendered\":\"error: run %s\"}}}}\\n' \"$n\" \"$n\"\n\
             exit 101\n",
            log = log.display(),
            signal = signal.display()
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
    // Each check says it started through a FIFO. The test holds the FIFO
    // open for reading and writing, so a check's write never blocks.
    assert!(Command::new("mkfifo")
        .arg(&signal)
        .status()
        .unwrap()
        .success());
    let fifo = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&signal)
        .unwrap();
    let (signalled, starts) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead as _;
        for line in std::io::BufReader::new(fifo).lines().map_while(Result::ok) {
            if signalled.send(line).is_err() {
                break;
            }
        }
    });

    let (reported, reports) = mpsc::channel();
    let background = rawr_cli::cargo::Background::start_with(
        cargo.into_os_string(),
        sandbox.path.join("Cargo.toml"),
        Duration::from_millis(200),
        move |status| drop(reported.send(status)),
    );
    // The launch check starts at once. Three saves stop it and start one.
    let first = starts.recv().expect("a check starts");
    assert_eq!(first, "1");
    for _ in 0..3 {
        background.saved();
        std::thread::sleep(Duration::from_millis(20));
    }

    let status = reports.recv().unwrap();
    let rawr_cli::cargo::Status::Finished(outcome) = status else {
        panic!("{status:?}");
    };
    assert_eq!(outcome.errors, ["error: run 2"]);
    assert!(reports.recv_timeout(Duration::from_millis(1500)).is_err());
    drop(background);
    assert_eq!(lines("started").len(), 2);
    assert_eq!(lines("finished"), ["2"]);
}

/// Marked modules named `switch`, `import` and `import::print`, words the
/// old engine read as keywords, are ordinary names: Rawr opens their files
/// and runs the crate as `cargo run` does.
#[test]
fn marked_modules_named_as_old_engine_keywords_run_as_cargo_runs_them() {
    let Some(cargo) = std::env::var_os("CARGO") else {
        panic!("run under cargo: $CARGO names the cargo to test with");
    };
    let sandbox = Sandbox::new("marked-names");
    let root = copy("rs-marker-names", &sandbox.work());

    let native = Command::new(&cargo)
        .args(["run", "--quiet", "--offline"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", sandbox.path.join("target"))
        .env("RUSTFLAGS", "--deny warnings")
        .output()
        .unwrap();
    let run = sandbox.rawr(&["run", "rs-marker-names"]);

    assert_success(&native);
    assert_eq!(stdout(&native), "1 2 20 true\n42\n");
    assert_success(&run);
    assert_eq!(stdout(&run), stdout(&native));
}

/// A marked crate's Cargo.toml dependencies are its Rust imports.
#[test]
fn a_marked_crate_imports_its_cargo_dependencies() {
    let sandbox = Sandbox::new("marked-deps");
    for name in ["rs-marker-rand", "rs-marker-chrono", "rs-marker-palette"] {
        let root = copy(name, &sandbox.work());
        let graph = graph(&root, None).unwrap();
        assert!(graph.declares_rust_import(), "{name}");
        assert!(graph.entry_package().rust_modules().is_empty(), "{name}");
    }
}

/// A marked crate in a Cargo workspace inherits its dependencies and
/// edition from the workspace root, as Cargo resolves them.
#[test]
fn a_marked_workspace_member_inherits_its_dependencies() {
    let sandbox = Sandbox::new("marked-workspace");
    let root = copy("rs-marker-workspace", &sandbox.work());

    let graph = graph(&root.join("app"), None).unwrap();

    assert!(graph.declares_rust_import());
}

/// The `rawr` attribute crate: Rawr passes over `#[preview]` and
/// `#[rawr::preview]`, binds nothing for `use rawr::preview;` and imports
/// nothing from the crate. rustc accepts the same files through the
/// crate without a warning. A function only previews call is not dead code,
/// and neither is a function a preview names as its seed.
#[test]
fn the_rawr_attribute_crate_builds_under_cargo_and_means_nothing_to_rawr() {
    let Some(cargo) = std::env::var_os("CARGO") else {
        panic!("run under cargo: $CARGO names the cargo to test with");
    };
    let sandbox = Sandbox::new("marked-attributes");
    let root = copy("rs-marker-attributes", &sandbox.work());
    let attributes = Path::new(env!("CARGO_MANIFEST_DIR")).join("../macros");
    let manifest = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace(
            "\"../../../../macros\"",
            &format!("{:?}", attributes.display().to_string()),
        ),
    )
    .unwrap();

    let graph = graph(&root, None).unwrap();
    assert!(!graph.declares_rust_import());

    let output = Command::new(&cargo)
        .args(["run", "--quiet", "--offline"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", sandbox.path.join("target"))
        .env("RUSTFLAGS", "--deny warnings")
        .output()
        .unwrap();
    assert_success(&output);
    assert_eq!(stdout(&output), "hello 3 items\n");
}

/// A file a package lists as a `[[bin]]` runs as that package's binary,
/// by its path and by `--bin`, with the package's profile: under
/// `panic = "abort"` its panic aborts, as the binary Cargo builds does
/// (#1175, #1176). A file the package doesn't list still runs alone, and
/// its panic is Rust's status 101.
#[cfg(unix)]
#[test]
fn a_bin_target_runs_as_its_packages_binary_with_its_profile() {
    use std::os::unix::process::ExitStatusExt as _;

    let Some(cargo) = std::env::var_os("CARGO") else {
        panic!("run under cargo: $CARGO names the cargo to test with");
    };
    let sandbox = Sandbox::new("marked-bin-target");
    let root = sandbox.work().join("lessons");
    fs::create_dir_all(root.join("exercises")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lessons\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [[bin]]\nname = \"clippy3\"\npath = \"exercises/clippy3.rs\"\n\n\
         [[bin]]\nname = \"clippy1\"\npath = \"exercises/clippy1.rs\"\n\n\
         [profile.dev]\npanic = \"abort\"\n\n[workspace]\n",
    )
    .unwrap();
    let program = "// rawr\nfn main() {\n    println!(\"before\");\n    \
                   let empty: Option<i32> = None;\n    empty.unwrap();\n}\n";
    fs::write(root.join("exercises/clippy3.rs"), program).unwrap();
    fs::write(root.join("exercises/alone.rs"), program).unwrap();
    // Another binary: the package's own entry is ambiguous, so the run is
    // the file's, not the directory's.
    fs::write(root.join("exercises/clippy1.rs"), "// rawr\nfn main() {}\n").unwrap();
    let bin = Path::new(&cargo).parent().unwrap();
    let target = sandbox.path.join("target");

    let built = Command::new(&cargo)
        .args(["build", "--quiet", "--offline", "--bin", "clippy3"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    assert_success(&built);
    let native = Command::new(target.join("debug/clippy3")).output().unwrap();
    assert_eq!(native.status.signal(), Some(6), "{}", stderr(&native));
    assert_eq!(stdout(&native), "before\n");

    let rawr = |arguments: &[&str]| {
        Command::new(sandbox.path.join("bin/rawr"))
            .args(arguments)
            .current_dir(&root)
            .env("PATH", bin)
            .env("CARGO", &cargo)
            .env("CARGO_TARGET_DIR", &target)
            .env("RAWR_HOME", sandbox.path.join("home"))
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap()
    };
    for arguments in [
        &["run", "exercises/clippy3.rs"][..],
        &["run", "--bin", "clippy3"],
    ] {
        let run = rawr(arguments);
        // SIGABRT, as a shell gives it.
        assert_eq!(
            run.status.code(),
            Some(128 + 6),
            "{arguments:?}: {}",
            stderr(&run)
        );
        assert_eq!(stdout(&run), "before\n");
        assert!(
            stderr(&run).contains("called `Option::unwrap()` on a `None` value"),
            "{}",
            stderr(&run)
        );
    }
    let alone = rawr(&["run", "exercises/alone.rs"]);
    assert_eq!(alone.status.code(), Some(101), "{}", stderr(&alone));

    let unknown = rawr(&["run", "--bin", "clippy4"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert_eq!(
        stderr(&unknown),
        "error: no bin target named `clippy4`\navailable bin targets:\n    clippy1\n    clippy3\n"
    );
}

/// A package's binary under `src/bin/`, with its own module beside it,
/// runs the marked modules of the library, as the binary at `src/main.rs`
/// does (#1116).
#[test]
fn a_marked_module_of_the_library_runs_from_a_binary_under_src_bin() {
    let sandbox = Sandbox::new("marked-lib-src-bin");
    let root = copy("rs-marker-lib", &sandbox.work());
    let scores = root.join("src/bin/scores");
    fs::create_dir_all(&scores).unwrap();
    fs::rename(root.join("src/main.rs"), scores.join("main.rs")).unwrap();
    fs::rename(root.join("src/rounds.rs"), scores.join("rounds.rs")).unwrap();

    let graph = graph(&root, None).unwrap();

    let package = graph.entry_package();
    assert_eq!(package.name(), "rs_marker_lib");
    let bin = package.bin().expect("the binary runs the program");
    assert_eq!(bin.root().path(), scores.join("main.rs"));
    let modules = bin
        .modules()
        .iter()
        .map(|module| (module.modules().join("::"), module.path().to_path_buf()))
        .collect::<Vec<_>>();
    assert_eq!(modules, [("rounds".to_owned(), scores.join("rounds.rs"))]);
}
