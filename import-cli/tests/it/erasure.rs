//! A script type a native generic fills as `rawr::Value` keeps its type
//! (D513, review 1, 2, 3, 8 and 9): a native `TypeId` cast to another script
//! type, or to another instance of a generic one, is refused where the
//! value comes back, a closure native code feeds a value of another type
//! refuses it, a `Clone` bound is checked on the script type, native
//! `Clone` and `Debug` are the script type's own, and a value holding a
//! native one passes through whole. The generated runner's own runtime
//! tests run last. Build the driver first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::copy;

/// `rawr-import run` on the `erasure` fixture with `main` as its program:
/// whether it succeeded, its stdout and its stderr. The cases share one
/// root, so the native crate builds once.
fn run(main: &str) -> (bool, String, String) {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let root = directory.join("erasure");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/erasure"),
        &root,
    );
    fs::write(root.join("src/main.rs"), format!("// rawr\n{main}")).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

/// A case that must run and print `expected`.
fn runs(main: &str, expected: &str) {
    let (success, stdout, stderr) = run(main);
    assert!(success, "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout, expected, "{main}\nstderr:\n{stderr}");
}

/// A case that must stop, saying `said` and printing nothing.
fn refused(main: &str, said: &str) {
    let (success, stdout, stderr) = run(main);
    assert!(!success, "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        !stdout.contains("wrong"),
        "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains(said),
        "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

const TYPES: &str = "
#[derive(Debug, Clone)]
struct Envelope<T> {
    value: T,
}

struct Ticket {
    number: i64,
}

struct Order {
    count: i64,
}
";

// The cases run in one test, one after another, on one root.
#[test]
fn a_script_type_filling_a_native_generic_keeps_its_type() {
    // Review 1: a cast to the same instance works; one to another
    // instance of the generic, or another script type, is refused.
    runs(
        &format!(
            "{TYPES}
fn main() {{
    let same: Option<Envelope<i64>> = natives::cast(Envelope {{ value: 1 }});
    println!(\"same {{}}\", same.map(|envelope| envelope.value).unwrap_or(-1));
}}
"
        ),
        "same 1\n",
    );
    refused(
        &format!(
            "{TYPES}
fn main() {{
    let other: Option<Envelope<String>> = natives::cast(Envelope {{ value: 1_i64 }});
    if let Some(envelope) = other {{
        println!(\"wrong {{}}\", envelope.value.len());
    }}
}}
"
        ),
        "not its type",
    );
    refused(
        &format!(
            "{TYPES}
fn main() {{
    let order: Option<Order> = natives::cast(Ticket {{ number: 3 }});
    if let Some(order) = order {{
        println!(\"wrong {{}}\", order.count);
    }}
}}
"
        ),
        "not its type",
    );
    // Review 2: a closure native code feeds a cast value checks it.
    runs(
        &format!(
            "{TYPES}
fn main() {{
    let number = natives::apply(Ticket {{ number: 3 }}, |ticket: Ticket| ticket.number);
    println!(\"applied {{number}}\");
}}
"
        ),
        "applied 3\n",
    );
    refused(
        &format!(
            "{TYPES}
fn main() {{
    let length = natives::apply(Envelope {{ value: 3_i64 }}, |envelope: Envelope<String>| {{
        envelope.value.len() as i64
    }});
    println!(\"wrong {{length}}\");
}}
"
        ),
        "not its type",
    );
    // Review 3: `T: Clone` is checked on the script type, as rustc does.
    runs(
        &format!(
            "{TYPES}
fn main() {{
    let (first, second) = natives::duplicate(Envelope {{ value: 2 }});
    println!(\"duplicated {{}} {{}}\", first.value, second.value);
}}
"
        ),
        "duplicated 2 2\n",
    );
    refused(
        &format!(
            "{TYPES}
fn main() {{
    let (first, second) = natives::duplicate(Ticket {{ number: 2 }});
    println!(\"wrong {{}} {{}}\", first.number, second.number);
}}
"
        ),
        "Clone",
    );
    // Review 8: native `Clone` and `Debug` are the script type's own.
    runs(
        &format!(
            "{TYPES}
struct Counter {{
    count: i64,
}}

impl Clone for Counter {{
    fn clone(&self) -> Self {{
        Counter {{ count: self.count + 1 }}
    }}
}}

fn main() {{
    let (copy, original) = natives::duplicate(Counter {{ count: 1 }});
    println!(\"cloned {{}} {{}}\", copy.count, original.count);
    println!(\"{{}}\", natives::show(Envelope {{ value: Some(2) }}));
}}
"
        ),
        "cloned 2 1\nEnvelope { value: Some(2) }\n",
    );
    // Review 9: a script value holding a native one passes through whole.
    runs(
        "
struct Holder {
    resource: natives::Resource,
}

fn main() {
    let holder = natives::identity(Holder { resource: natives::Resource::new(3) });
    println!(\"held {}\", holder.resource.id());
}
",
        "held 3\n",
    );
    // The runner's runtime tests, which only a generated runner compiles:
    // a handle the protocol gave out stays alive (review 5), and a lent
    // value given back on a thread without the arena, or while a native
    // call holds it, returns to its slot (review 6, 7).
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("erasure");
    let test = Command::new("cargo")
        .current_dir(&root)
        .args([
            "test",
            "-q",
            "--manifest-path",
            "target/rawr/runner/Cargo.toml",
        ])
        .args(["--", "native_arena_tests"])
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&test.stdout);
    assert!(
        test.status.success() && stdout.contains("6 passed"),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );
}
