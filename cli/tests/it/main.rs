//! `rawr` end to end. Each test runs a copy of `rawr` from its own
//! directory, with stand-in `rawr-<name>` binaries beside it as the test
//! needs, and an empty `PATH`, so nothing installed on the machine is found.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

mod marked;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A directory holding `bin/rawr`, a Rawr home and the programs of a test.
struct Sandbox {
    path: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rawr-base-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("bin")).unwrap();
        fs::create_dir_all(path.join("work")).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_rawr"), path.join("bin/rawr")).unwrap();
        Self {
            path: fs::canonicalize(path).unwrap(),
        }
    }

    fn work(&self) -> PathBuf {
        self.path.join("work")
    }

    /// A stand-in `rawr-<name>` (shell builtins only: `PATH` is empty)
    /// printing `manifest` when asked for it,
    /// and otherwise `<name>: <arguments>`, exiting 3. Each time it is asked
    /// for its manifest it adds a line to `asked-<name>`.
    fn binary(&self, name: &str, manifest: &str) {
        let path = self.path.join("bin").join(format!("rawr-{name}"));
        let asked = self.path.join(format!("asked-{name}"));
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = --rawr-manifest ]; then\n  echo asked >> '{}'\n  printf '%s\\n' '{manifest}'\n  exit 0\nfi\necho \"{name}: $*\"\nexit 3\n",
                asked.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn asked(&self, name: &str) -> usize {
        fs::read_to_string(self.path.join(format!("asked-{name}")))
            .map_or(0, |text| text.lines().count())
    }

    /// `rawr` with nothing reachable but its own directory: no cargo,
    /// rustc or store.
    fn rawr(&self, arguments: &[&str]) -> Output {
        let missing = self.path.join("missing");
        Command::new(self.path.join("bin/rawr"))
            .args(arguments)
            .current_dir(self.work())
            .env("PATH", "")
            .env("HOME", &missing)
            .env("CARGO_HOME", &missing)
            .env("RUSTUP_HOME", &missing)
            .env("RAWR_HOME", self.path.join("home"))
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO")
            .env_remove("RAWR_VERBOSE")
            .output()
            .unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
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

const FAKE: &str = "format = 1\nname = \"rawr-fake\"\ncrates = [\"fakecrate\"]\ncommands = [\"fly\"]";

#[test]
fn rawr_runs_the_cart_with_no_build_and_leaves_its_directory_as_it_was() {
    let sandbox = Sandbox::new("cart");
    fs::write(
        sandbox.work().join("cart.rs"),
        include_str!("../../../examples/scripts/cart.rs"),
    )
    .unwrap();
    let before = tree(&sandbox.work());

    let run = sandbox.rawr(&["run", "cart.rs"]);
    let checked = sandbox.rawr(&["check", "cart.rs"]);

    assert_success(&run);
    assert_eq!(stdout(&run), "total: 27\n");
    assert!(run.stderr.is_empty());
    assert_success(&checked);
    assert!(stdout(&checked).starts_with("Checked "));
    assert_eq!(tree(&sandbox.work()), before);
}

#[test]
fn rawr_prints_a_final_value_and_main_output() {
    let sandbox = Sandbox::new("output");
    fs::write(sandbox.work().join("value.rs"), "let x = 20.0 + 7.0;\nx\n").unwrap();
    fs::write(
        sandbox.work().join("main.rs"),
        "fn main() {\n    print!(\"a\");\n    println!(\"{}\", 1 + 1);\n    eprintln!(\"to stderr\");\n}\n",
    )
    .unwrap();

    let value = sandbox.rawr(&["run", "value.rs"]);
    let main = sandbox.rawr(&["main.rs"]);

    assert_success(&value);
    assert_eq!(stdout(&value), "27.0\n");
    assert_success(&main);
    assert_eq!(stdout(&main), "a2\n");
    assert_eq!(stderr(&main), "to stderr\n");
}

#[test]
fn main_returning_err_fails_like_rust() {
    let sandbox = Sandbox::new("err");
    fs::write(
        sandbox.work().join("err.rs"),
        "fn main() -> Result<(), String> {\n    Err(String::from(\"boom\"))\n}\n",
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "err.rs"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(stderr(&output), "Error: \"boom\"\n");
}

#[test]
fn a_panic_exits_101_like_rust() {
    let sandbox = Sandbox::new("panic");
    fs::write(
        sandbox.work().join("panic.rs"),
        "fn main() {\n    println!(\"before\");\n    panic!(\"boom {}\", 1);\n}\n",
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "panic.rs"]);

    assert_eq!(output.status.code(), Some(101));
    assert_eq!(stdout(&output), "before\n");
    assert!(stderr(&output).contains("boom 1"), "{}", stderr(&output));
}

#[test]
fn type_errors_are_the_same_diagnostic_for_check_and_run() {
    let sandbox = Sandbox::new("type-error");
    let source = sandbox.work().join("wrong.rs");
    fs::write(&source, "fn main() {\n    let x: i64 = \"text\";\n}\n").unwrap();

    let checked = sandbox.rawr(&["check", "wrong.rs"]);
    let run = sandbox.rawr(&["run", "wrong.rs"]);

    assert!(!checked.status.success());
    assert!(!run.status.success());
    assert_eq!(checked.stderr, run.stderr);
    let text = stderr(&checked);
    assert!(text.starts_with("error[RAWR-RESOLVE-"), "{text}");
    assert!(text.contains(&format!("{}:2:", source.display())), "{text}");
}

#[test]
fn a_program_using_iced_without_rawr_iced_says_how_to_install_it() {
    let sandbox = Sandbox::new("missing-iced");
    fs::write(
        sandbox.work().join("app.rs"),
        "use iced::widget::text;\npub fn main() -> iced::Result { Ok(()) }\n",
    )
    .unwrap();

    for command in ["run", "check"] {
        let output = sandbox.rawr(&[command, "app.rs"]);
        assert_eq!(output.status.code(), Some(1));
        let text = stderr(&output);
        assert!(
            text.contains("this program uses `iced`, which rawr-iced serves, and it is not installed"),
            "{text}"
        );
        assert!(
            text.contains("cargo +nightly-2026-07-11 install --locked --git https://github.com/airstrike/rawr rawr-iced-cli"),
            "{text}"
        );
        assert!(!text.contains("layer"), "{text}");
    }
}

/// `rawr new` writes a plain crate with no dependencies, which runs and
/// checks on the base language with no build and no extension.
#[test]
fn a_new_crate_runs_on_the_base() {
    let sandbox = Sandbox::new("new");

    let created = sandbox.rawr(&["new", "hello"]);
    let run = sandbox.rawr(&["run", "hello"]);
    let checked = sandbox.rawr(&["check", "hello"]);

    assert_success(&created);
    let root = sandbox.work().join("hello");
    assert!(stdout(&created).starts_with(&format!("Created a Rawr crate at {}", root.display())));
    let files = tree(&root);
    assert_eq!(
        files.keys().cloned().collect::<Vec<_>>(),
        [PathBuf::from("Cargo.toml"), PathBuf::from("src/main.rs")]
    );
    let source = String::from_utf8(files[Path::new("src/main.rs")].clone()).unwrap();
    assert!(source.starts_with("// rawr\n"));
    assert!(!source.contains("iced"));
    assert_success(&run);
    assert_eq!(
        stdout(&run),
        "1. Hello, rawr!\n2. Hello, rawr!\n3. Hello, rawr!\n"
    );
    assert_success(&checked);
    assert_eq!(
        stdout(&checked),
        format!("Checked {}\n", root.join("src/main.rs").display())
    );
    // A new crate refuses to overwrite one.
    let again = sandbox.rawr(&["new", "hello"]);
    assert_eq!(again.status.code(), Some(1));
    assert!(stderr(&again).contains("refusing non-empty project target"), "{}", stderr(&again));
}

/// A template comes from the installed binary whose manifest offers it,
/// with the same arguments; one that is not installed says how to get it.
#[test]
fn a_template_is_written_by_the_binary_offering_it() {
    let sandbox = Sandbox::new("template");
    sandbox.binary(
        "shapes",
        "format = 1\nname = \"rawr-shapes\"\ntemplates = [\"shapes\"]",
    );

    let handed = sandbox.rawr(&["new", "app", "--template", "shapes"]);
    let missing = sandbox.rawr(&["new", "app", "--template", "iced"]);
    let unknown = sandbox.rawr(&["new", "app", "--template", "nothing"]);
    let help = sandbox.rawr(&["new", "--help"]);

    assert_eq!(handed.status.code(), Some(3));
    assert_eq!(stdout(&handed), "shapes: new app --template shapes\n");
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        stderr(&missing).starts_with("error: the `iced` template comes with rawr-iced, which is not installed. Install it with:\n"),
        "{}",
        stderr(&missing)
    );
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("no installed binary offers a template named `nothing`"));
    assert_success(&help);
    assert!(stdout(&help).contains("shapes       from rawr-shapes"), "{}", stdout(&help));
    assert!(!sandbox.work().join("app").exists());
}

/// A program with Rust dependencies goes to rawr-import; without it,
/// `rawr` says how to install it.
#[test]
fn a_program_with_rust_dependencies_without_rawr_import_says_how_to_install_it() {
    let sandbox = Sandbox::new("missing-import");
    let root = sandbox.work().join("deps");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"deps\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nitoa = \"1\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(root.join("src/main.rs"), "// rawr\nfn main() {}\n").unwrap();

    let output = sandbox.rawr(&["run", "deps"]);

    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(
        text.contains("this program uses Rust dependencies in Cargo.toml, which rawr-import serves, and it is not installed"),
        "{text}"
    );
    assert!(text.contains(" rawr-import-cli"), "{text}");
    assert!(!text.contains("iced"), "{text}");
}

#[test]
fn a_command_from_rawr_iced_says_how_to_install_it() {
    let sandbox = Sandbox::new("missing-preview");

    let preview = sandbox.rawr(&["preview"]);
    let unknown = sandbox.rawr(&["bogus"]);

    assert_eq!(preview.status.code(), Some(1));
    assert!(
        stderr(&preview).starts_with("error: `rawr preview` comes with rawr-iced, which is not installed. Install it with:\n  rustup toolchain install nightly-2026-07-11"),
        "{}",
        stderr(&preview)
    );
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown command `bogus`"));
}

#[test]
fn rawr_forwards_a_command_and_hands_off_a_program_to_the_binary_serving_it() {
    let sandbox = Sandbox::new("hand-off");
    sandbox.binary("fake", FAKE);
    fs::write(
        sandbox.work().join("app.rs"),
        "use fakecrate::thing;\nfn main() { let x = 1; }\n",
    )
    .unwrap();

    let forwarded = sandbox.rawr(&["fly", "high"]);
    let handed = sandbox.rawr(&["run", "app.rs"]);
    let checked = sandbox.rawr(&["check", "app.rs"]);

    assert_eq!(forwarded.status.code(), Some(3));
    assert_eq!(stdout(&forwarded), "fake: fly high\n");
    assert_eq!(handed.status.code(), Some(3));
    assert_eq!(stdout(&handed), "fake: run app.rs\n");
    assert_eq!(stdout(&checked), "fake: check app.rs\n");
    // `rawr` kept the program's parse for the binary it handed it to.
    let cells = fs::read_dir(sandbox.work().join("target/rawr/parse"))
        .unwrap()
        .count();
    assert_eq!(cells, 1);
    // Asked for its manifest once, however many commands ran.
    assert_eq!(sandbox.asked("fake"), 1);
}

#[test]
fn rawr_says_how_it_loaded_a_program_when_asked() {
    let sandbox = Sandbox::new("verbose");
    sandbox.binary("fake", FAKE);
    fs::write(sandbox.work().join("app.rs"), "use fakecrate::thing;\n").unwrap();

    let first = sandbox.rawr_verbose(&["check", "app.rs"]);
    let second = sandbox.rawr_verbose(&["check", "app.rs"]);

    let first = stderr(&first);
    assert!(first.contains("rawr: parsed 1 sources, read 0 from parse cells"), "{first}");
    assert!(first.contains("rawr: wrote 1 parse cells"), "{first}");
    assert!(first.contains("rawr-fake serves this program; handing it over"), "{first}");
    let second = stderr(&second);
    assert!(second.contains("rawr: parsed 0 sources, read 1 from parse cells"), "{second}");
}

#[test]
fn a_program_two_binaries_claim_is_a_diagnostic() {
    let sandbox = Sandbox::new("two");
    sandbox.binary("fake", FAKE);
    sandbox.binary(
        "other",
        "format = 1\nname = \"rawr-other\"\ncrates = [\"othercrate\"]",
    );
    fs::write(
        sandbox.work().join("app.rs"),
        "use fakecrate::a;\nuse othercrate::b;\nfn main() {}\n",
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "app.rs"]);

    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("this program uses `fakecrate` and `othercrate`"), "{text}");
}

#[test]
fn help_lists_what_installed_binaries_add() {
    let sandbox = Sandbox::new("help");
    let alone = sandbox.rawr(&["--help"]);
    sandbox.binary("fake", FAKE);
    let with = sandbox.rawr(&["--help"]);

    assert_success(&alone);
    assert!(stdout(&alone).contains("rawr-iced is not installed. It serves programs that use `iced`. Install it with:"));
    assert!(stdout(&alone).contains("rawr new <path>"));
    assert!(
        stdout(&with).contains("From rawr-fake"),
        "{}\n{}\n{:?}",
        stdout(&with),
        stderr(&with),
        fs::read_to_string(sandbox.path.join("home/manifests.toml"))
    );
    assert!(stdout(&with).contains("rawr fly"));
    for text in [stdout(&alone), stdout(&with)] {
        assert!(!text.contains("layer"), "{text}");
    }
}

impl Sandbox {
    fn rawr_verbose(&self, arguments: &[&str]) -> Output {
        let missing = self.path.join("missing");
        Command::new(self.path.join("bin/rawr"))
            .args(arguments)
            .current_dir(self.work())
            .env("PATH", "")
            .env("HOME", &missing)
            .env("CARGO_HOME", &missing)
            .env("RAWR_HOME", self.path.join("home"))
            .env_remove("CARGO_TARGET_DIR")
            .env("RAWR_VERBOSE", "1")
            .output()
            .unwrap()
    }
}

#[test]
fn inline_modules_and_cfg_test_items_build_as_rustc_does() {
    let sandbox = Sandbox::new("inline-modules");
    let project = sandbox.work().join("shapes");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"shapes\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
    fs::write(
        project.join("src/main.rs"),
        r#"// rawr
const BASE: i64 = 40;

fn main() {
    use shapes::inner::double;
    println!("{} {}", double(shapes::area(2)), shapes::label());
}

mod shapes {
    use super::*;

    pub fn area(side: i64) -> i64 {
        side * side + BASE
    }

    #[cfg(not(test))]
    pub fn label() -> String {
        String::from("built")
    }

    #[cfg(test)]
    pub fn label() -> String {
        String::from("tested")
    }

    pub mod inner {
        pub fn double(x: i64) -> i64 {
            x * 2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use not_a_dependency::Anything;

    #[test]
    fn area() { assert_eq!(shapes::area(1), 41); }
}
"#,
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "shapes"]);

    assert_success(&output);
    assert_eq!(stdout(&output), "88 built\n");
}

#[test]
fn compile_time_macros_read_the_project_as_rustc_does() {
    let sandbox = Sandbox::new("compile-time");
    let project = sandbox.work().join("macros");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/greeting.txt"), "hello\n").unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"macros\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
    fs::write(
        project.join("src/main.rs"),
        r#"// rawr
use std::f32::consts::TAU;
use std::f64::consts;

const NAME: &str = concat!("ra", "wr", '-', 1, 2.5, true);

fn main() {
    let greeting = include_str!("greeting.txt");
    let bytes = include_bytes!("greeting.txt");
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/greeting.txt");
    println!("{} {} {} {}", NAME, greeting.trim(), bytes.len(), bytes[0]);
    println!("{} {}", path.ends_with("/src/greeting.txt"), path.starts_with(manifest));
    println!("{:.5} {:.5} {:.5} {:.3}", std::f64::consts::PI, consts::E, TAU, core::f64::consts::SQRT_2);
    println!("{}", stringify!(a + b));
}
"#,
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "macros"]);

    assert_success(&output);
    // The same program built by cargo prints this.
    assert_eq!(
        stdout(&output),
        "rawr-12.5true hello 6 104\ntrue true\n3.14159 2.71828 6.28319 1.414\na + b\n"
    );
}

#[test]
fn a_rawr_file_or_rawr_toml_says_how_to_convert_it() {
    let sandbox = Sandbox::new("legacy");
    fs::write(sandbox.work().join("app.rawr"), "fn main() {}\n").unwrap();
    let app = sandbox.work().join("old");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(app.join("rawr.toml"), "[package]\nname = \"old\"\n").unwrap();
    fs::write(app.join("src/main.rawr"), "fn main() {}\n").unwrap();

    let file = sandbox.rawr(&["run", "app.rawr"]);
    let manifest = sandbox.rawr(&["check", "old/rawr.toml"]);
    let directory = sandbox.rawr(&["run", "old"]);

    assert_eq!(file.status.code(), Some(1));
    let text = stderr(&file);
    assert!(text.contains("is a `.rawr` file. Rawr reads `.rs` files now."), "{text}");
    assert!(text.contains("Rename the file to `.rs` and put `// rawr` on its first line."), "{text}");
    assert_eq!(manifest.status.code(), Some(1));
    let text = stderr(&manifest);
    assert!(text.contains("rawr.toml` is no longer read."), "{text}");
    assert!(text.contains("Put the project in a Cargo crate whose `src/main.rs` starts with `// rawr`."), "{text}");
    assert_eq!(directory.status.code(), Some(1));
    let text = stderr(&directory);
    assert!(text.contains("no Rawr project found"), "{text}");
    assert!(text.contains("rawr.toml` is no longer read."), "{text}");
}
