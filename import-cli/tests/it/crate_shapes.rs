//! Shapes of a crate's public API that a marked crate reaches as Rust does,
//! each in a module of the fixture's `natives` crate and a main in its
//! `mains/`. Each main prints what `cargo run` prints under `rawr-import
//! run`. Build the driver first, as for `plain`.

use std::process::Command;

use crate::support::Fixture;

static SHAPES: Fixture = Fixture::new("crate-shapes");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A lane of the fixture with `mains/{main}.rs` as its `src/main.rs`, run
/// by `cargo run` and by `rawr-import run`: both print `expected`.
fn runs_as_cargo_does(main: &str, expected: &str) {
    let program = SHAPES.read(&format!("mains/{main}.rs"));
    let root = SHAPES.lease(&[("src/main.rs", &program)]);
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    assert_eq!(text(&native.stdout), expected);

    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
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

/// A method or field a crate's type lacks is its `Deref` target's, as
/// rustc's probe finds it: another of the crate's types, a `String` and a
/// `Vec`. A `&mut self` method is reached through the type's `DerefMut`,
/// and a `&Name` is the `&str` a function takes (#762).
#[test]
fn a_crate_types_deref_gives_its_targets_members() {
    runs_as_cargo_does("deref", "clicks 2 0\n4\n6 FERRIS FERRIS\n3 Some(3) 6\n");
}

/// A borrow of a crate struct's field that derefs to `[u8]`, as iced's
/// `&screenshot.rgba`, is the `&[u8]` a function or a `let` takes, and a
/// borrow of the struct, whose other fields are read after.
#[test]
fn a_borrowed_crate_field_derefs_without_moving_its_owner() {
    runs_as_cargo_does("deref_field", "7\n4 2\n4 Some(1)\n4\n");
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

/// A generic impl's function whose named callable parameter `F` takes or
/// gives what no script closure can, as tokio's `MappedMutexGuard::map`
/// (`F: FnOnce(&mut T) -> &mut U`) and cosmic text's `with_buffer_mut`
/// (`F: FnOnce(&mut Borrowed<Buffer>) -> T`) are, has no binding, and the
/// module's other items stay importable. So does a method whose closure
/// gives a future parameter, as warp's `Ws::on_upgrade` (`F: FnOnce(WebSocket)
/// -> U, U: Future`) is. Each used to fail the import with "unknown type
/// parameter" (T313).
#[test]
fn an_uncallable_named_callable_parameter_leaves_the_crate_importable() {
    runs_as_cargo_does("guard", "3\n");
}

/// A trait method generic over `T` is a member of the opaque `impl Trait`
/// that a function generic over its own `T` gives, as warp's `form<T>()`
/// gives an `impl Filter` with `untuple_one<T>`. The two `T`s used to be
/// declared twice in one signature, which failed the crate's import. A
/// member that gives a type holding the opaque type (`Filter::and`), and a
/// type's method that gives an opaque type (`Source::filter`, as image's
/// `Reader::into_decoder`), used to declare an opaque type under a function
/// or a type, which isn't a module, and failed the import too.
#[test]
fn an_opaque_types_members_import() {
    runs_as_cargo_does("opaque", "7\nvalue 8\n");
}

/// A crate's trait impl on a type with a const parameter, as palette's
/// `impl<T> Lookup<u8, T> for ArrayTable<256>` is, leaves the crate
/// importable: no facade type writes a const argument, so the impl's header
/// is left out. An impl on `Hsl`, which rustc reads as `Hsl<Srgb>`, keeps
/// its header with the default filled in. The const one used to give
/// `Table` no arguments and stop the import.
#[test]
fn a_trait_impl_on_a_const_argument_leaves_the_crate_importable() {
    runs_as_cargo_does("table", "3\n");
}
