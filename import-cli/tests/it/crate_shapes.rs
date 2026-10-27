//! Shapes of a crate's public API that a marked crate reaches as Rust does,
//! each in a module of the fixture's `natives` crate and a main in its
//! `mains/`. Each main prints what `cargo run` prints under `rawr-import
//! run`. Build the driver first, as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::copy;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crate-shapes");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The fixture with `mains/{main}.rs` as its `src/main.rs`, run by `cargo
/// run` and by `rawr-import run`: both print `expected`.
fn runs_as_cargo_does(main: &str, expected: &str) {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("crate-shapes-{main}"));
    let _ = fs::remove_dir_all(root.join("src"));
    let _ = fs::remove_dir_all(root.join("natives/src"));
    copy(Path::new(FIXTURE), &root);
    fs::copy(
        root.join("mains").join(format!("{main}.rs")),
        root.join("src/main.rs"),
    )
    .unwrap();
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    assert_eq!(text(&native.stdout), expected);

    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
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

/// A generic type's builder methods that take `impl Fn(..) + 'a`, as
/// iced's `Button::on_press_with` and `Button::style` do, take script
/// closures, and the crate's other items stay importable. The style
/// closure gets a native `&Theme` and `Status` it reads and matches.
#[test]
fn impl_fn_parameters_of_a_generic_builder_take_script_closures() {
    runs_as_cargo_does("button", "pressed 2\nwidths 3 6\n");
}

/// A crate's `pub use Alignment::Center;` re-exports the variant, as
/// `iced::Center` is, and `use Enum::Variant;` imports one: each name is
/// the variant in expressions and patterns.
#[test]
fn a_re_exported_variant_and_a_used_variant_are_the_enums_variants() {
    runs_as_cargo_does("alignment", "Center\ncenter end start\ntrue\ntrue\n");
}

/// A crate's type aliases, as `iced::Result` (`std::result::Result<(),
/// Error>`) and `iced::Element` (a generic type with a default parameter)
/// are, name their targets in headers and bodies, through a `use` and by
/// their path, with each parameter not written its default.
#[test]
fn a_crates_type_aliases_name_their_targets_with_defaults() {
    runs_as_cargo_does("aliases", "message 7\nErr(Failed(\"no\"))\n");
}

/// A module and a function of the same name (`mod text` and `fn text`),
/// which a glob brings in and another module re-exports with a glob, as
/// `iced::widget` does: `use widget::text` binds both, so `text(..)` calls
/// the function and `text::Text` names the module's type, in an annotation
/// too.
#[test]
fn a_module_and_a_function_of_one_name_are_both_reached() {
    runs_as_cargo_does(
        "text",
        "count at 20\nother at 16\nfull at 16\nbase at 16\n1\n",
    );
}

/// A generic trait's provided method, as iced's `Widget<Message, ..>`'s
/// `_boxed`, is a member of each type that implements the trait, through an
/// implementation whose parameter its type doesn't name (`impl<Message>
/// Widget<Message> for Space`): called as a method, through the trait's
/// `use`d name and through its path, as `column!` calls it, with the
/// parameter inferred from what the call gives.
#[test]
fn a_generic_traits_provided_method_is_each_implementors() {
    runs_as_cargo_does("provided", "4\n5\n6\n");
}
