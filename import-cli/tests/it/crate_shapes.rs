//! Shapes of a crate's public API that a marked crate reaches as Rust does,
//! each in a module of the fixture's `natives` crate and a main in its
//! `mains/`. Each main prints what `cargo run` prints under `rawr-import
//! run`. Build the driver first, as for `plain`.

use std::process::Command;

use crate::support::Fixture;

static SHAPES: Fixture = Fixture::new("crate-shapes");
static OWN_LIBRARY: Fixture = Fixture::new("own-library");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A lane of the fixture with `mains/{main}.rs` as its `src/main.rs`, run
/// by `cargo run` and by `rawr-import run`: both print `expected`.
fn runs_as_cargo_does(main: &str, expected: &str) {
    let program = SHAPES.read(&format!("mains/{main}.rs"));
    lane_runs_as_cargo_does(&SHAPES.lease(&[("src/main.rs", &program)]), expected);
}

/// The project at `root`, run by `cargo run` and by `rawr-import run`:
/// both print `expected`.
fn lane_runs_as_cargo_does(root: &std::path::Path, expected: &str) {
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    assert_eq!(text(&native.stdout), expected);

    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(root)
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

/// A crate's `pub fn print`, `pub mod import` with a `fn switch`, a field
/// `this` and a method `call` are reached by those names: words the old
/// engine read as keywords are ordinary names, as in Rust.
#[test]
fn crate_items_named_as_old_engine_keywords_are_ordinary_names() {
    runs_as_cargo_does("names", "printed once\ntrue\n6\n");
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

/// A crate type's methods are function values, as iced's
/// `pick_list(.., Theme::ALL, Theme::to_string)` names one (#319): its
/// own `&self`, `self` and `&mut self` methods, its `Display`'s
/// `to_string` and a trait's method by the trait's path, `<T as Trait>::`
/// and the type's, passed where a crate function takes `impl Fn(&T) ->
/// String`, coerced to a `fn` pointer, and called by path. A script
/// function of `&Look` fills the same slot.
#[test]
fn a_crate_types_methods_are_function_values() {
    runs_as_cargo_does(
        "method_paths",
        "[\"Light\", \"Dark\"]\n[\"<Light>\", \"<Dark>\"]\n[\"light\", \"dark\"] [200, 30] [200, 30]\n[100, -70] [100, -70] [100, -70]\ndark light Dark\n",
    );
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
    runs_as_cargo_does("opaque", "7\nvalue 8\n2\n");
}

/// A function's opaque `impl Number` whose trait's supertrait binds
/// `Add::Output` to `Self`, as num-traits' `Num` does: the opaque type's
/// `Output` is the opaque type itself, left as its projection, and its
/// members import. Converting it used to convert the opaque type again
/// inside itself until the stack overflowed (#1262).
#[test]
fn an_opaque_type_whose_supertrait_binds_self_imports() {
    runs_as_cargo_does("numbers", "7\n");
}

/// A crate's opaque `impl Iterator<Item = u32>`, as a function gives it
/// and as a generic type's method gives one: `next`, a `for` loop and the
/// iterator's adaptors (`count`, `map`, `filter`) pull from it as rustc's
/// do, through `Iterator::next`.
#[test]
fn a_crates_opaque_iterator_is_iterated() {
    runs_as_cargo_does("iterated", "Some(3)\nstep 2\nstep 1\n4\n[6, 4, 2]\n6 2\n");
}

/// swash's `Setting<T>`: its impl for `u16` and its impl for `f32` each
/// give an opaque iterator from `parse_list`, two opaque types, each
/// declared with its own `next`, so the crate imports, as every iced
/// program's text stack needs.
#[test]
fn a_types_impls_that_give_opaque_iterators_by_one_name_import() {
    runs_as_cargo_does("settings", "Setting(3) 4\n");
}

/// A type whose method gives an opaque iterator, reached through a module
/// that re-exports the type's module, as ratatui's
/// `macros::ratatui_core::text::Line` is, by a program that never calls the
/// method: the iterator's `next` is declared at each path its owner is. It
/// used to be declared at the second path without its owner, which failed
/// the import.
#[test]
fn an_opaque_iterator_of_a_re_exported_type_imports() {
    runs_as_cargo_does("iterated_again", "3\n");
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

/// A number on an operator's left with a crate's type on its right, as
/// iced's ferris writes `(rotation + step) % (2.0 * Radians::PI)`, selects
/// the crate's impl of the operator for that number (`impl Mul<Turn> for
/// f32`), beside the type's own `Mul<f32>` and `Mul`: its `Output`, and the
/// kind a literal takes from it, as rustc selects (#974).
#[test]
fn a_number_on_the_left_selects_the_crates_impl_for_it() {
    runs_as_cargo_does(
        "operators",
        "Turn(1.4) Turn(14.0) Turn(48.999996)\nTurn(0.8)\n9.300000011920929 Turn(1.5)\nTurn(1.05)\n",
    );
}

/// A float literal whose only constraint is a bound one float type meets,
/// as iced's progress_bar writes `Animation::new(0.)` with lilt's `Float`
/// implemented for `bool` and `f32`, is that type, as rustc selects the
/// one impl it fits (#1028), in a closure too.
#[test]
fn a_literal_takes_the_one_impl_its_bound_allows() {
    runs_as_cargo_does("animation", "f32 0\nf32 0.25\n");
}

/// A value that isn't `Send`, as rand's `ThreadRng` holds an `Rc`, lent
/// `&mut` to a call, generic or not, as the_matrix's
/// `distribution.sample(&mut rng)`: the call holds it on its thread, which
/// asks no `Send` (#957).
#[test]
fn a_value_that_stays_on_its_thread_needs_no_send() {
    runs_as_cargo_does("random", "[0, 2, 1, 2, 1, 2, 1, 2, 0, 0, 1, 2] 0\n");
}

/// An impl's parameter that only a bound on an iterator's items fixes, as
/// rand's `WeightedIndex::new(weights)` has `I::Item: SampleBorrow<X>`:
/// an array of integer literals is `[i32; 4]`, so `X` is `i32`, as rustc
/// infers it, and the distribution's later `sample` names that type
/// (#1084).
#[test]
fn a_bound_on_an_iterators_items_fixes_the_impls_parameter() {
    runs_as_cargo_does("weighted", "i32 [0, 1, 0, 3, 0, 0]\n");
}

/// A script impl of a crate trait whose method takes a `&mut` of a type
/// with a lifetime, as geometry's `update(.., shell: &mut Shell<'_,
/// Message>)`: native code lends the shell for the call, and the script's
/// changes reach it, with no `Clone` (#957).
#[test]
fn a_mut_input_with_a_lifetime_is_lent_not_cloned() {
    runs_as_cargo_does("shell", "2 4\n");
}

/// A method whose own early-bound lifetime its output borrows a `&F` at,
/// as jiff's `strftime<'f, F: 'f + ?Sized + AsRef<[u8]>>(&self, format:
/// &'f F) -> Display<'f>` is (#1014): a literal fills `F` with `str`, and a
/// `&String` and a `&[u8]` fill it too. The call borrows its receiver for
/// the call alone and opens a scope frame that holds what the format
/// borrows, and the frame's value prints through its `Display`.
#[test]
fn an_output_borrows_a_formats_early_bound_lifetime() {
    runs_as_cargo_does("strftime", "09:05\nat 09 h\n05 past\n0905\n");
}

/// The format stays borrowed while what `strftime` gives lives, as rustc
/// reads its signature: rawr's own check refuses a change to it with
/// rustc's E0502 before anything runs.
#[test]
fn a_formats_borrow_lasts_as_long_as_its_output() {
    let program = "// rawr\nuse natives::lifetimes::Clock;\n\nfn main() {\n    let clock = Clock::new(9, 5);\n    let mut format = String::from(\"%H\");\n    println!(\"start\");\n    let shown = clock.strftime(&format);\n    format.push_str(\"x\");\n    println!(\"{shown}\");\n}\n";
    let root = SHAPES.lease(&[("src/main.rs", program)]);
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
    let stdout = text(&run.stdout);
    let stderr = text(&run.stderr);
    assert!(
        !run.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!stdout.contains("start"), "{stdout}");
    assert!(stderr.contains("E0502"), "{stderr}");
}

/// A crate's function of open's `that_in_background` shape, which takes an
/// `impl AsRef<OsStr>` and gives a thread's `JoinHandle<io::Result<()>>`:
/// a `&str` and a `String` fill the argument, and the program ignores the
/// handle or joins it for what the crate's thread gave (#1266).
#[test]
fn a_crates_join_handle_is_joined_or_ignored() {
    runs_as_cargo_does("opened", "true\ntrue\n");
}

/// A path into a crate none of whose items the program can use yet, as
/// `opener::that_if_given` gives an `Option<JoinHandle<..>>`: the crate is
/// a module, and rawr says why the item can't be used, as it does in a
/// crate with other items. It used to say rustc's E0433 for the crate, as
/// for one the program doesn't depend on (#1267).
#[test]
fn a_crate_with_no_usable_item_says_why_its_item_cant_be_used() {
    let program = "// rawr\n\nfn main() {\n    let _ = opener::that_if_given(Some(\"https://iced.rs\"));\n}\n";
    let root = SHAPES.lease(&[("src/main.rs", program)]);
    let directory = crate::support::scratch();
    let check = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stderr = text(&check.stderr);
    assert!(!check.status.success(), "{stderr}");
    assert!(!stderr.contains("E0433"), "{stderr}");
    assert!(
        stderr.contains(
            "`that_if_given` can't be used yet: rawr doesn't run `std::thread::JoinHandle` here in a crate's signature yet"
        ),
        "{stderr}"
    );
}

/// A package's marked binary names the package's library by its crate
/// name, as Cargo builds the binary with the library as a dependency
/// (#1164): it calls the library's `run`, generic over the binary's own
/// state and message types, with its own functions, and uses the
/// library's type by a `use` and by its path.
#[test]
fn a_marked_binary_names_its_packages_library_by_the_crate_name() {
    lane_runs_as_cargo_does(&OWN_LIBRARY.lease(&[]), "floor 2 after 4 trips\ntrue\n");
}

/// A generic trait's provided method that gives an opaque `impl Total`,
/// whose supertrait `Add<Output = Self>` binds `Output` to the opaque type
/// itself, is a member of a plain type and of a generic one, each of
/// which calls it and the opaque type's method. The crate imports: `Self`
/// there is the opaque type, not the type the method is a member of
/// (#1278).
#[test]
fn a_provided_methods_opaque_output_whose_supertrait_binds_self() {
    runs_as_cargo_does("doubling", "6\n4\n");
}

/// A crate's recursive enum, whose group holds `&'static [Self]`, is built
/// from literals in a static and in a call and read by the crate's
/// functions. A facade names another facade's type by its fingerprint,
/// never by walking that facade's contract at each mention (#1309).
#[test]
fn a_crates_recursive_enum_of_a_static_slice_of_itself() {
    runs_as_cargo_does("pages", "5\nBuilt 2\nExamples\n");
}

/// A crate's unit struct, re-exported by another module, is a value of its
/// type, as `badge::Secondary` is in a component crate: copied, compared,
/// formatted by its derives, matched as a pattern, passed where a method
/// takes `impl Into<StyleFn>` with `impl From<Secondary> for StyleFn`, and
/// converted by `.into()`. A tuple struct's constructor is a function
/// value passed to `map` (#1317).
#[test]
fn a_crates_unit_struct_is_a_value_and_its_tuple_constructor_a_function() {
    runs_as_cargo_does(
        "markers",
        "Secondary true true\nSecondary: secondary 9\nGhost: ghost 5\nmatched\n[Wrap(1), Wrap(2)]\n",
    );
}

/// futures_util's forwarding impls: `impl<S, Item> Sink<Item> for Wrap<S>`
/// and `impl<T, F> Stream for Immediate<F> where F: Future<Output = T>`,
/// where `Forward<S, A>`'s `Future::Output` is a `Result<(), E>` whose `E`
/// only its where clause fixes. Each impl is declared, none is left out,
/// and the program calls each type's own method. `Forward`'s `E`, a
/// parameter of its own impl, used to be read into the `Output = T` bound
/// as a parameter of `Immediate`'s impl, which has no such parameter, and
/// left that impl out (#1295).
#[test]
fn a_forwarding_impls_parameters_are_its_own() {
    runs_as_cargo_does("forwarding", "1\n2\n");
    let program = SHAPES.read("mains/forwarding.rs");
    let root = SHAPES.lease(&[("src/main.rs", &program)]);
    let directory = crate::support::scratch();
    let check = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env("RAWR_VERBOSE", "1")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let stderr = text(&check.stderr);
    assert!(check.status.success(), "{stderr}");
    assert!(!stderr.contains("left out"), "{stderr}");
}

/// A crate's `Slot<'a, M>` with `From<&'a str>`, `From<String>` and a
/// `From` for its own icon type: `Into::<Slot<'_, _>>::into(child)` in a
/// crate macro's array of children, as a component crate's `badge!` and
/// `row!` write it, picks the impl for each child's type, a literal taking
/// `From<&'a str>` and a `String` `From<String>`. So do `"x".into()` and
/// `From::from("x")` with an annotated target and
/// `<Slot<_> as From<_>>::from(..)`. The literal's `Slot` borrows it, so
/// its call opens a scope frame ahead of the statement, and the icon
/// before it in the array is bound ahead of that frame, as Rust runs it
/// first (#1326).
#[test]
fn a_qualified_into_picks_the_crates_from_impl_for_its_argument() {
    runs_as_cargo_does(
        "slots",
        "icon 12 | str Verified | string Verified\nstr into\nstr from | string qualified\n",
    );
}
