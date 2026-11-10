//! A crate's trait objects in marked code (D872): a struct literal whose
//! field is a `&'static dyn Trait` built from a static's place, numbers, a
//! `bool` and crate values given where a call takes `&dyn Trait`, a
//! `Box<dyn Trait>` a crate gives and one the program makes, and tracing's
//! callsite, whose metadata holds it as an `Identifier(&'static dyn Site)`
//! a macro builds at the call, a cycle of statics. Native code makes every
//! object, and the program prints what cargo's build prints. Build the
//! driver first, as for `islands`.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::support::copy;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/trait-objects");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn rawr_import(command: &str, root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg(command)
        .arg(root)
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

#[test]
fn trait_objects_of_a_crates_trait_are_made_by_native_code_as_cargo_makes_them() {
    let root = crate::support::scratch().join("trait-objects");
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    let check = rawr_import("check", &root);
    assert!(
        check.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&check.stdout),
        text(&check.stderr)
    );

    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    assert_eq!(
        text(&native.stdout),
        "held 9 true\n\
         static 9\n\
         int 5\n\
         u8 1007\n\
         bool 1\n\
         square 4\n\
         boxed 16\n\
         made 6\n\
         tick tick back true\n\
         tick tick back true\n"
    );

    let run = rawr_import("run", &root);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), text(&native.stdout));
}
