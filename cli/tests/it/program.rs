//! What a program on the base language gets from `rawr run` (D500.1): the
//! arguments after `--`, stdin, and a restart when a save changes it.

use std::fs;
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::{assert_success, stderr, stdout, Sandbox};

/// `rawr` started with piped stdin, stdout and stderr, and its lines.
struct Running {
    child: Child,
    lines: mpsc::Receiver<String>,
    seen: Vec<String>,
}

impl Running {
    fn start(sandbox: &Sandbox, arguments: &[&str]) -> Self {
        let mut child = Command::new(sandbox.path.join("bin/rawr"))
            .args(arguments)
            .current_dir(sandbox.work())
            .env("PATH", "")
            .env("RAWR_HOME", sandbox.path.join("home"))
            .env_remove("CARGO")
            .env_remove("RAWR_VERBOSE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (send, lines) = mpsc::channel();
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        for (stream, prefix) in [
            (Box::new(out) as Box<dyn std::io::Read + Send>, ""),
            (Box::new(err), "stderr: "),
        ] {
            let send = send.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if send.send(format!("{prefix}{line}")).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            child,
            lines,
            seen: Vec::new(),
        }
    }

    /// The next line `accept` takes, waiting up to `limit`.
    fn until(&mut self, limit: Duration, accept: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    self.seen.push(line.clone());
                    if accept(&line) {
                        return line;
                    }
                }
                Err(_) => panic!("timed out; output so far:\n{}", self.seen.join("\n")),
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

const TICKS: &str = r#"// rawr
const VERSION: i64 = 1;

fn main() {
    let mut tick = 0;
    while tick < 400 {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(50) {}
        tick += 1;
        println!("tick {tick} version {VERSION}");
    }
}
"#;

#[test]
fn a_save_restarts_a_running_program_and_a_broken_one_leaves_it_running() {
    let sandbox = Sandbox::new("program-restart");
    let file = sandbox.work().join("ticks.rs");
    fs::write(&file, TICKS).unwrap();
    let mut run = Running::start(&sandbox, &["run", "ticks.rs"]);
    run.until(Duration::from_secs(30), |line| line == "tick 3 version 1");

    // A save that doesn't compile leaves the program running.
    fs::write(&file, TICKS.replace("VERSION: i64 = 1", "VERSION: i64 = ")).unwrap();
    run.until(Duration::from_secs(30), |line| {
        line.starts_with("stderr: error")
    });
    run.until(Duration::from_secs(30), |line| line.ends_with("version 1"));

    fs::write(&file, TICKS.replace("VERSION: i64 = 1", "VERSION: i64 = 2")).unwrap();
    run.until(Duration::from_secs(30), |line| {
        line == "stderr: rawr: the source changed; restarting the program"
    });
    run.until(Duration::from_secs(30), |line| line == "tick 1 version 2");
}

const ECHO: &str = r#"// rawr
use std::io::BufRead;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    println!("{} arguments", args.len());
    for arg in args.iter().skip(1) {
        println!("arg {arg}");
    }
    let mut first = String::new();
    std::io::stdin().read_line(&mut first).unwrap();
    println!("first {}", first.trim_end());
    for line in std::io::stdin().lock().lines() {
        println!("line {}", line.unwrap());
    }
}
"#;

#[test]
fn a_program_gets_the_arguments_after_two_dashes_and_stdin() {
    let sandbox = Sandbox::new("program-arguments");
    fs::write(sandbox.work().join("echo.rs"), ECHO).unwrap();
    let mut run = Running::start(
        &sandbox,
        &["run", "echo.rs", "--", "one", "two words", "--"],
    );
    let mut stdin = run.child.stdin.take().unwrap();
    stdin.write_all(b"alpha\nbeta\ngamma\n").unwrap();
    drop(stdin);
    let status = run.child.wait().unwrap();
    let lines: Vec<_> = run.lines.try_iter().collect();
    assert!(status.success(), "{lines:?}");
    assert_eq!(
        lines
            .iter()
            .filter(|line| !line.starts_with("stderr: "))
            .collect::<Vec<_>>(),
        [
            "4 arguments",
            "arg one",
            "arg two words",
            "arg --",
            "first alpha",
            "line beta",
            "line gamma",
        ]
    );
}

#[test]
fn a_program_sees_its_name_as_cargo_run_gives_it() {
    let sandbox = Sandbox::new("program-name");
    let root = sandbox.work().join("named");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"named\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nfn main() {\n    println!(\"{}\", std::env::args().next().unwrap());\n}\n",
    )
    .unwrap();

    let run = sandbox.rawr(&["run", "named", "--", "x"]);

    assert_success(&run);
    let expected = Path::new(&fs::canonicalize(&root).unwrap())
        .join("target/debug/named")
        .display()
        .to_string();
    assert_eq!(stdout(&run), format!("{expected}\n"), "{}", stderr(&run));
}

/// `examples/grep` under `rawr run`, on stdin and on files, prints what its
/// `cargo build --release` binary prints.
#[test]
fn the_grep_example_reads_its_flags_files_and_stdin() {
    let sandbox = Sandbox::new("program-grep");
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/grep");
    let root = sandbox.work().join("grep");
    fs::create_dir_all(root.join("src")).unwrap();
    for file in ["Cargo.toml", "src/main.rs", "src/search.rs"] {
        fs::copy(example.join(file), root.join(file)).unwrap();
    }
    let menu = "apple\nBanana\ncherry pie\napple pie\n";
    fs::write(sandbox.work().join("menu.txt"), menu).unwrap();
    fs::write(sandbox.work().join("veg.txt"), "carrot\npie of potato\n").unwrap();

    let mut run = Running::start(&sandbox, &["run", "grep", "--", "-n", "-i", "PIE"]);
    let mut stdin = run.child.stdin.take().unwrap();
    stdin.write_all(menu.as_bytes()).unwrap();
    drop(stdin);
    assert!(run.child.wait().unwrap().success());
    let lines: Vec<_> = run
        .lines
        .try_iter()
        .filter(|line| !line.starts_with("stderr: "))
        .collect();
    assert_eq!(lines, ["3:cherry pie", "4:apple pie"]);

    let files = sandbox.rawr(&["run", "grep", "--", "pie", "menu.txt", "veg.txt"]);
    assert_success(&files);
    assert_eq!(
        stdout(&files),
        "menu.txt:cherry pie\nmenu.txt:apple pie\nveg.txt:pie of potato\n"
    );
    let count = sandbox.rawr(&["run", "grep", "--", "-c", "-v", "pie", "menu.txt"]);
    assert_eq!(stdout(&count), "2\n");
    let usage = sandbox.rawr(&["run", "grep", "--"]);
    assert_eq!(usage.status.code(), Some(1));
    assert!(
        stderr(&usage).contains("Error: \"usage: grep [-i] [-n] [-c] [-v] PATTERN [FILE...]\""),
        "{}",
        stderr(&usage)
    );
}
