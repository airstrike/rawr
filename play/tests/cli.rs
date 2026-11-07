//! The `rawr-play` CLI runs a plain program to its output as JSON, and
//! answers an iced or async snippet with its message.

use std::process::Command;

/// Run `source` through the CLI with `args`, the result on stdout.
fn run_stdin(source: &str, args: &[&str]) -> std::process::Output {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .arg("-")
        .args(args)
        .args(["-o", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
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

fn text_result(source: &str, args: &[&str]) -> rawr_play::Output {
    let result = run_stdin(source, args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout.first(), Some(&b'{'), "a text result is JSON");
    rawr_play::Output::from_json(&result.stdout).unwrap()
}

#[test]
fn cli_writes_a_plain_programs_output_as_json() {
    let cart = text_result(include_str!("../../examples/scripts/cart.rs"), &[]);
    assert_eq!((cart.text().as_str(), cart.status), ("total: 27\n", 0));
    let expression = text_result("vec![1, 2, 3].iter().sum::<i64>()", &[]);
    assert_eq!(expression.text(), "6\n");
    let failed = text_result("fn main() -> Result<(), String> {\n    eprintln!(\"oops\");\n    Err(\"no\".to_string())\n}\n", &[]);
    assert_eq!(
        (failed.text().as_str(), failed.status),
        ("oops\nError: \"no\"\n", 1)
    );
}

#[test]
fn cli_stops_an_endless_plain_program_at_the_time_limit() {
    let looped = text_result(
        "fn main() {\n    let mut n = 0;\n    while n >= 0 { n = n + 1 - 1; }\n}\n",
        &["--time-ms", "200"],
    );
    assert!(looped.limit && looped.status == 1, "{looped:?}");
}

#[test]
fn cli_reports_a_plain_compile_error_on_stderr() {
    let result = run_stdin("fn main() {\n    let x: i64 = \"s\";\n}\n", &[]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("play.rs:2:"));
}

#[test]
fn cli_refuses_an_async_main_with_no_runtime_as_rustc_does() {
    let result = run_stdin("async fn main() {\n    println!(\"hi\");\n}\n", &[]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("error[E0752]: `main` function is not allowed to be `async`"),
        "{stderr}"
    );
    assert!(stderr.contains("play.rs:1:1"), "{stderr}");
}

/// The playground's examples (examples.ts) run as rustc's build of each
/// does: the same stdout and exit status, or for one rustc refuses, its
/// error at its place. tests/fixtures/rust/expected holds what rustc's
/// build printed (`generate.sh` writes it).
#[test]
fn cli_runs_the_playgrounds_examples_as_rustc_does() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = root.join("tests/fixtures/rust");
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    };
    // Each example examples.ts imports and lists.
    let menu = read(root.join("deploy/cloudflare/src/examples.ts"));
    let examples: Vec<&str> = menu
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("import ")?;
            let (binding, path) = line.split_once(" from \"../../../tests/fixtures/rust/")?;
            let name = path.strip_suffix(".rs\";")?;
            assert!(
                menu.contains(&format!("code: {binding} }}")),
                "examples.ts imports {name}.rs but doesn't list it"
            );
            Some(name)
        })
        .collect();
    let mut files: Vec<String> = std::fs::read_dir(&fixtures)
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".rs").map(str::to_owned)
        })
        .collect();
    files.sort();
    let mut listed: Vec<String> = examples.iter().map(|name| (*name).to_owned()).collect();
    listed.sort();
    assert_eq!(
        files, listed,
        "every Rust fixture is an example in examples.ts"
    );

    for name in examples {
        let source = read(fixtures.join(format!("{name}.rs")));
        let expected = fixtures.join("expected");
        let status: i32 = read(expected.join(format!("{name}.status")))
            .trim()
            .parse()
            .unwrap();
        let result = run_stdin(&source, &["--time-ms", "20000"]);
        let stderr = String::from_utf8_lossy(&result.stderr);
        if result.status.success() {
            let output = rawr_play::Output::from_json(&result.stdout).unwrap();
            let stdout: String = output
                .chunks
                .iter()
                .filter(|(stream, _)| *stream == rawr_play::plain::Stream::Stdout)
                .map(|(_, text)| text.as_str())
                .collect();
            let wanted = read(expected.join(format!("{name}.stdout")));
            assert_eq!(
                (stdout.as_str(), output.status),
                (wanted.as_str(), status),
                "{name}"
            );
            continue;
        }
        // A program rustc refuses: its first error, at the same place.
        let rustc = read(expected.join(format!("{name}.stderr")));
        assert_eq!(
            status, 1,
            "{name} built under rustc but not in rawr: {stderr}"
        );
        assert!(result.stdout.is_empty(), "{name}");
        let first = |text: &str, prefix: &str| {
            text.lines()
                .map(str::trim_start)
                .find(|line| line.starts_with(prefix))
                .map(str::to_owned)
        };
        assert_eq!(
            first(&stderr, "error"),
            first(&rustc, "error"),
            "{name}: {stderr}"
        );
        let place = |text: &str| {
            let line = first(text, "--> ")?;
            // Where it starts: rawr's adds where it ends.
            let (_, place) = line.split_once(".rs:")?;
            place.split('-').next().map(str::to_owned)
        };
        assert_eq!(place(&stderr), place(&rustc), "{name}: {stderr}");
    }
}

/// rawr-web's snippets (web/tests/fixtures, `.snippet` since most are
/// statements rustfmt can't read) print what rustc's build of each prints. `node web/tests/run.ts` checks rawr-web against the same
/// files, so the browser and the server agree.
#[test]
fn cli_runs_rawr_webs_fixtures_as_rustc_does() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../web/tests/fixtures");
    let mut names: Vec<std::path::PathBuf> = std::fs::read_dir(&fixtures)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "snippet")
        })
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for path in names {
        let source = std::fs::read_to_string(&path).unwrap();
        let wanted = std::fs::read_to_string(path.with_extension("stdout")).unwrap();
        let output = text_result(&source, &[]);
        assert_eq!(
            (output.text().as_str(), output.status),
            (wanted.as_str(), 0),
            "{}",
            path.display()
        );
    }
}
