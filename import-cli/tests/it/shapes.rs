//! T204: a marked crate reaches items with the shapes upstream iced examples
//! reach, as Rust has them. A dependency's `Rgb<S = Srgb, T = f32>` with a
//! `PhantomData` field is built with `..Rgb::default()` (color_palette);
//! `&(f64, f64)` and `&[(f64, f64)]` (multitouch) and `&[u8]` (screenshot)
//! are borrows that leave the script's values usable; a native module's
//! `Result<(Self, Vec<Entry>), Error>` crosses whole (changelog). An item
//! rawr can't bind says why. Build the driver first, as for `plain`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use crate::support::Fixture;

static SHAPES: Fixture = Fixture::new("native-shapes");

fn rawr_import(command: &str, root: &Path) -> Output {
    let directory = crate::support::scratch();
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg(command)
        .arg(root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn upstream_example_shapes_run_and_print_what_cargo_run_prints() {
    let root = SHAPES.lease(&[]);
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    let expected = text(&native.stdout);
    assert!(expected.contains("rgb 0.25 0.5 1\n"), "{expected}");
    assert!(expected.contains("inside Some(2) of 3\n"), "{expected}");
    assert!(expected.contains("checksum 1014\n"), "{expected}");
    assert!(expected.contains("entry 4 beta\n"), "{expected}");

    let run = rawr_import("run", &root);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(
        text(&run.stdout),
        expected,
        "stderr:\n{}",
        text(&run.stderr)
    );
}

#[test]
fn an_item_rawr_cant_bind_says_why() {
    let root = SHAPES.lease(&[]);
    fs::write(
        root.join("src/main.rs"),
        "// rawr\nuse shapes::checksum;\n\nfn main() {\n    let mut bytes: Vec<u8> = vec![1, 2];\n    println!(\"{}\", checksum(&bytes));\n    shapes::clear(&mut bytes);\n}\n",
    )
    .unwrap();
    let check = rawr_import("check", &root);
    let stderr = text(&check.stderr);
    assert!(!check.status.success(), "{stderr}");
    assert!(stderr.contains("error[E0425]"), "{stderr}");
    assert!(
        stderr.contains(
            "`clear` can't be used yet: its signature has a slice type where one isn't supported yet"
        ),
        "{stderr}"
    );
}
