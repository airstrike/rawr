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
        sandbox.work().join("cart.rawr"),
        include_str!("../../../examples/cart.rawr"),
    )
    .unwrap();
    let before = tree(&sandbox.work());

    let run = sandbox.rawr(&["run", "cart.rawr"]);
    let checked = sandbox.rawr(&["check", "cart.rawr"]);

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
    fs::write(sandbox.work().join("value.rawr"), "let x = 20.0 + 7.0;\nx\n").unwrap();
    fs::write(
        sandbox.work().join("main.rawr"),
        "fn main() {\n    print!(\"a\");\n    println!(\"{}\", 1 + 1);\n    eprintln!(\"to stderr\");\n}\n",
    )
    .unwrap();

    let value = sandbox.rawr(&["run", "value.rawr"]);
    let main = sandbox.rawr(&["main.rawr"]);

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
        sandbox.work().join("err.rawr"),
        "fn main() -> Result<(), String> {\n    Err(String::from(\"boom\"))\n}\n",
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "err.rawr"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(stderr(&output), "Error: \"boom\"\n");
}

#[test]
fn a_panic_exits_101_like_rust() {
    let sandbox = Sandbox::new("panic");
    fs::write(
        sandbox.work().join("panic.rawr"),
        "fn main() {\n    println!(\"before\");\n    panic!(\"boom {}\", 1);\n}\n",
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "panic.rawr"]);

    assert_eq!(output.status.code(), Some(101));
    assert_eq!(stdout(&output), "before\n");
    assert!(stderr(&output).contains("boom 1"), "{}", stderr(&output));
}

#[test]
fn type_errors_are_the_same_diagnostic_for_check_and_run() {
    let sandbox = Sandbox::new("type-error");
    let source = sandbox.work().join("wrong.rawr");
    fs::write(&source, "fn main() {\n    let x: i64 = \"text\";\n}\n").unwrap();

    let checked = sandbox.rawr(&["check", "wrong.rawr"]);
    let run = sandbox.rawr(&["run", "wrong.rawr"]);

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
        sandbox.work().join("app.rawr"),
        "use iced::widget::text;\npub fn main() -> iced::Result { Ok(()) }\n",
    )
    .unwrap();

    for command in ["run", "check"] {
        let output = sandbox.rawr(&[command, "app.rawr"]);
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

#[test]
fn a_command_from_rawr_iced_says_how_to_install_it() {
    let sandbox = Sandbox::new("missing-preview");

    let preview = sandbox.rawr(&["preview"]);
    let as_rust = sandbox.rawr(&["check", "--as-rust"]);
    let unknown = sandbox.rawr(&["bogus"]);

    assert_eq!(preview.status.code(), Some(1));
    assert!(
        stderr(&preview).starts_with("error: `rawr preview` comes with rawr-iced, which is not installed. Install it with:\n  rustup toolchain install nightly-2026-07-11"),
        "{}",
        stderr(&preview)
    );
    assert!(stderr(&as_rust).contains("`rawr check --as-rust` comes with rawr-iced"));
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown command `bogus`"));
}

#[test]
fn rawr_forwards_a_command_and_hands_off_a_program_to_the_binary_serving_it() {
    let sandbox = Sandbox::new("hand-off");
    sandbox.binary("fake", FAKE);
    fs::write(
        sandbox.work().join("app.rawr"),
        "use fakecrate::thing;\nfn main() { let x = 1; }\n",
    )
    .unwrap();

    let forwarded = sandbox.rawr(&["fly", "high"]);
    let handed = sandbox.rawr(&["run", "app.rawr"]);
    let checked = sandbox.rawr(&["check", "app.rawr"]);

    assert_eq!(forwarded.status.code(), Some(3));
    assert_eq!(stdout(&forwarded), "fake: fly high\n");
    assert_eq!(handed.status.code(), Some(3));
    assert_eq!(stdout(&handed), "fake: run app.rawr\n");
    assert_eq!(stdout(&checked), "fake: check app.rawr\n");
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
    fs::write(sandbox.work().join("app.rawr"), "use fakecrate::thing;\n").unwrap();

    let first = sandbox.rawr_verbose(&["check", "app.rawr"]);
    let second = sandbox.rawr_verbose(&["check", "app.rawr"]);

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
        sandbox.work().join("app.rawr"),
        "use fakecrate::a;\nuse othercrate::b;\nfn main() {}\n",
    )
    .unwrap();

    let output = sandbox.rawr(&["run", "app.rawr"]);

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
    assert!(stdout(&alone).contains("rawr-iced is not installed. Install it with:"));
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
    fs::write(
        project.join("src/main.rawr"),
        r#"const BASE: i64 = 40;

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
    fs::write(
        project.join("src/main.rawr"),
        r#"use std::f32::consts::TAU;
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
