//! A crate's trait objects in marked code (D872): a struct literal whose
//! field is a `&'static dyn Trait` built from a static's place, numbers, a
//! `bool` and crate values given where a call takes `&dyn Trait`, a
//! `Box<dyn Trait>` a crate gives and one the program makes, and tracing's
//! callsite, whose metadata holds it as an `Identifier(&'static dyn Site)`
//! a macro builds at the call, a cycle of statics. Native code makes every
//! object, and the program prints what cargo's build prints. So it does of
//! objects lent in a slice of `Option`s, which the value a call makes of
//! them borrows, as tracing-core's `value_set_all` gives its `ValueSet`,
//! with `format_args!` among them, made in the frame that lends them, and
//! in a block an older edition's macro makes, whose frame is its caller
//! statement's (#1561). Marked code calls an object's methods, of one it
//! holds, one native code boxed and one it coerced (#1593), and gives
//! `&format_args!(..)` where a call takes `&dyn Value`, as rustc coerces
//! it, and a slice of objects to a call that doesn't keep them (#1592).
//! An `Rc<dyn Trait>` and an `Arc<dyn Trait>` the program makes cross as
//! native code's own (#1304).
//! Build the driver first, as for `islands`.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::support::copy;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

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

/// What the fixture `name` prints, once `rawr-import check` passes it and
/// `rawr-import run` prints what `cargo run` does.
fn runs_as_cargo(name: &str) -> String {
    let root = crate::support::scratch().join(name);
    let _ = fs::remove_dir_all(root.join("src"));
    copy(&Path::new(FIXTURES).join(name), &root);
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

    let run = rawr_import("run", &root);
    assert!(
        run.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        text(&run.stdout),
        text(&run.stderr)
    );
    assert_eq!(text(&run.stdout), text(&native.stdout));
    text(&native.stdout)
}

#[test]
fn trait_objects_of_a_crates_trait_are_made_by_native_code_as_cargo_makes_them() {
    assert_eq!(
        runs_as_cargo("trait-objects"),
        "held 9 true\n\
         path 9\n\
         called 9\n\
         given 25 back 25\n\
         coerced 1003\n\
         static 9\n\
         int 5\n\
         u8 1007\n\
         bool 1\n\
         square 4\n\
         boxed 16\n\
         made 6\n\
         counted (25, 1)\n\
         shared 7\n\
         tick tick back true\n\
         tick tick back true\n"
    );
}

#[test]
fn trait_objects_lent_in_a_slice_are_borrowed_by_what_the_call_makes_of_them() {
    assert_eq!(
        runs_as_cargo("trait-object-slices"),
        "a=5 b=- c=(2, 3)\n\
         event a=5 b=- c=(2, 3)\n\
         event a=true b=- c=-\n\
         one 5\n\
         uncoerced 5\n\
         5,-,(2, 3)\n\
         5,-,(2, 3)\n\
         event a=n 5 b=5 c=x 5 y\n"
    );
}
