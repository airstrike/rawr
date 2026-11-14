//! Shapes of a crate's public API that a marked crate reaches as Rust does,
//! each in a module of the fixture's `natives` crate and a main in its
//! `mains/`. Each main prints what `cargo run` prints under `rawr-import
//! run`. Build the driver first, as for `plain`.

use std::process::Command;

use crate::support::{cargo_s_report, Fixture};

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

/// The same field's deref where the program names no path to its type,
/// as iced's screenshot reaches `Bytes` only as a field's (#1100).
#[test]
fn a_crate_field_of_a_type_no_path_names_derefs() {
    runs_as_cargo_does("deref_unnamed", "7\n");
}

/// A crate type's methods are function values, as iced's
/// `pick_list(.., Theme::ALL, Theme::to_string)` names one (#319): its
/// own `&self`, `self` and `&mut self` methods, its `Display`'s
/// `to_string` and a trait's method by the trait's path, `<T as Trait>::`
/// and the type's, passed where a crate function takes `impl Fn(&T) ->
/// String`, coerced to a `fn` pointer, and called by path. A script
/// function of `&Look` fills the same slot. The type's associated `const
/// ALL: [Look; 2]` and a function giving `[Look; 2]` are its values (#967),
/// and so is its `const NAMES: &'static [&'static str]` (#1626).
#[test]
fn a_crate_types_methods_are_function_values() {
    runs_as_cargo_does(
        "method_paths",
        "[\"Light\", \"Dark\"]\nDark [Light, Dark]\n[\"<Light>\", \"<Dark>\"]\n[\"light\", \"dark\"] [200, 30] [200, 30]\n[100, -70] [100, -70] [100, -70]\ndark light Dark\n[\"light\", \"dark\"] 2 light\n",
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

/// A struct literal and a struct pattern through a crate's type alias
/// build and match the aliased struct with the alias's arguments, as
/// wgpu's `DeviceDescriptor<'a> = wgt::DeviceDescriptor<Option<&'a
/// str>>` does (#1961). A `&self` method's `impl Future` that captures no
/// lifetime is a future the call makes, as wgpu's `request_device` is. An
/// alias that is its struct's only public name, as wgpu's
/// `SurfaceConfiguration`, has the struct's fields at its arguments. A
/// pattern reads its `Option<&str>` field, and a struct update fills the
/// rest from `Default::default()` (#1970).
#[test]
fn struct_literals_and_patterns_through_a_crates_type_alias() {
    runs_as_cargo_does(
        "described",
        "4 2\ngpu\nSome(\"cpu\") 1\n3\ntrue\n5 10\n10\n7\n",
    );
}

/// A `&self` method on a crate struct's field reads the field where it is,
/// as rustc borrows the place, so the struct stays whole: a `Vec` of a Copy
/// crate type, as wgpu's `capabilities.formats.iter()`, and an `Option` of
/// a crate struct (#1969).
#[test]
fn a_fields_shared_method_borrows_the_field_in_place() {
    runs_as_cargo_does("capabilities", "Srgb 2\ntrue\nSome(\"main\") 2\n2\n");
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

/// A crate function's opaque iterator that borrows the `&str` it takes, as
/// iced's `markdown::parse(&str) -> impl Iterator<Item = Item> + '_`
/// gives, lives beside the string it borrows, so `collect`, `next` and a
/// `for` loop pull from it as rustc's do (#1966).
#[test]
fn an_opaque_iterator_that_borrows_its_text_is_iterated() {
    runs_as_cargo_does(
        "parsed",
        "[Setting(1), Setting(2), Setting(3)]\nSome(Setting(4))\nSetting(6)\nSetting(7)\n",
    );
}

/// An associated function whose output keeps a `&'a T` and a `&'a mut U`
/// it takes at one early-bound lifetime, as iced's integration
/// `Scene::clear(&view, &mut encoder, color)` gives a `RenderPass<'a>`:
/// the output holds both places for the rest of its block, its drop
/// writes into the one it borrows exclusively, and the program uses that
/// one again after the block, as rustc's borrow ends there (#1973).
#[test]
fn a_value_keeps_an_exclusive_borrow_of_what_it_was_made_from() {
    runs_as_cargo_does(
        "passes",
        "frame\nclear frame to 7, draw 1 on frame, draw 2 on frame, end\n",
    );
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

/// A compound assignment with a number on the left and a crate's type on
/// the right, `y += turn` with the crate's `impl AddAssign<Turn> for f32`,
/// calls the impl with the number as its `&mut` place, on a local and on
/// a field, as rustc does, and so does a program's own impl (#987).
#[test]
fn a_number_on_the_left_of_a_compound_assignment_takes_the_crates_impl() {
    runs_as_cargo_does("compound", "1.5\n2.5\n6.5\n");
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

/// elevato's `Simulation` lends its `&Building` to the program's `update`
/// and `view`, as iced lends `&State` to `view`: the script function
/// borrows it for each call, with no `Clone` and no copy, and sees each
/// change native code made between calls (#1405). It reads the building
/// through its method, its `Display`, its `Debug` and its `PartialEq`, and
/// passes the same borrow to the crate's `summary(&Building) -> String`
/// and `picture(&Building) -> Picture<'a, M>`, as elevato's
/// `view(building)` (#1406). Its `advance` answers a `Command` made
/// during the call, which borrows nothing of the building, run by a
/// method given it and by one that keeps it, as elevato's `frame` runs
/// the `update` its `Simulation::new` kept: the kept function's native
/// calls reach the arena, and native code takes what it gives (#1432).
#[test]
fn a_native_borrow_lent_to_a_script_function_is_a_borrow_for_each_call() {
    runs_as_cargo_does(
        "lobby",
        "1 3 floors in the 3-storey building Building { floors: 3 } false\n\
         2 summary of 4 floors Building { floors: 4 } true\n\
         picture of 5 floors\n\
         3 5 floors in the 5-storey building Building { floors: 5 } false\n\
         picture of 6 floors\n\
         4 at 6 floors, 3 to go\n\
         5 at 7 floors, 2 to go\n\
         6 at 8 floors, 1 to go\n\
         delivered 4\n\
         8 at 10 floors, 2 to go\n\
         9 at 11 floors, 1 to go\n\
         delivered 3\n\
         12 at 14 floors, 1 to go\n\
         delivered 3\n",
    );
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

/// A widget generic over a script message and the `&str` content it holds,
/// invariant in its lifetime, made from a program's `String`, as iced's
/// `button(label)` makes a `Button<'_, Message, &str>`, and by a function
/// whose own signature borrows the label, which opens a scope frame: both
/// adapters compile, so the pins a lent widget borrows outlive the widget
/// (#1396).
#[test]
fn a_widget_lent_a_programs_string_drops_before_its_pins() {
    runs_as_cargo_does("pressable", "Delete off 1\nKeep off 1\ndeleted\nkept\n");
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

/// A `&self` method whose receiver's lifetime is the one its `&mut`
/// input's type carries, as integration's `Scene::draw<'a>(&'a self, &mut
/// RenderPass<'a>)`, lends its receiver to the value in the scope frame's
/// region for that value's life: the value keeps it, and the receiver is
/// changed again once the value has dropped (#1982).
#[test]
fn a_method_lends_its_receiver_into_what_its_mut_input_refers_to() {
    runs_as_cargo_does(
        "layers",
        "sea\nclear frame to 7, draw 1 on frame, layer sky, end\n",
    );
}

/// A `&self` method's opaque output that captures the receiver's
/// lifetime, as integration's `Controls::view(&self) -> impl Widget<..>`
/// does under edition 2024's rules, is held as a value that borrows its
/// receiver for as long as it lives: the receiver is changed again once the
/// value has dropped, or moved into a call that takes an `impl Trait`
/// (#1991).
#[test]
fn an_opaque_output_keeps_its_receiver_borrowed_while_it_lives() {
    runs_as_cargo_does("panels", "[sky]\n|[sea]|\n[land]\n");
}

/// The receiver `panel.view()` captures stays borrowed while the value
/// lives: rawr's own check refuses a `&mut` of it meanwhile with rustc's
/// E0502 before anything runs (#1991).
#[test]
fn a_receiver_an_opaque_output_captures_stays_borrowed() {
    let program = "// rawr\nuse natives::panels::{Panel, Show};\n\nfn main() {\n    let mut panel = Panel::new(\"sky\");\n    println!(\"start\");\n    let shown = panel.view();\n    panel.rename(\"sea\");\n    println!(\"{}\", shown.show());\n}\n";
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

/// A `&mut` a pattern binds fills a crate's `&dyn Trait` input, reborrowed
/// shared as rustc coerces it, as integration's `interface.update(window,
/// ..)` passes its `&mut Arc<Window>`.
#[test]
fn a_mut_binding_fills_a_crates_trait_object_input() {
    runs_as_cargo_does("windows", "4\n4\n");
}

/// A `for` loops over a crate's value by its `IntoIterator` impl, as
/// integration's `for message in messages` loops over iced's `shell::Bus`,
/// and a `&mut` of a crate's value made for the call fills a `&mut`
/// input, as its `&mut shell::Bus::new()` does.
#[test]
fn a_for_loops_over_a_crates_into_iterator() {
    runs_as_cargo_does("bags", "2\n3\n1\n");
}

/// A `for` loops over a crate's value whose items are the program's own
/// type, as integration's `for message in messages` loops over iced's
/// `shell::Bus<controls::Message>` (#1992).
#[test]
fn a_for_loops_over_a_crates_iterator_of_program_values() {
    runs_as_cargo_does("bagged", "Renamed(\"sea\")\nCleared\nCleared\n");
}

/// A public field of a struct the crate implements `Drop` for is read
/// where it is, as integration reads wgpu's `SurfaceTexture::texture`: the
/// runner never moves it out, which rustc refuses (E0509).
#[test]
fn a_field_of_a_dropped_struct_is_read_in_place() {
    runs_as_cargo_does("frames", "5\n");
}

/// The receiver `layer.draw(&mut pass)` lends `pass` stays borrowed while
/// `pass` lives, as rustc reads `draw<'a>(&'a self, &mut Pass<'a>)`:
/// rawr's own check refuses a `&mut` of it meanwhile with rustc's E0502
/// before anything runs (#1982).
#[test]
fn a_receiver_lent_into_a_mut_input_stays_borrowed() {
    let program = "// rawr\nuse natives::passes::{Encoder, Layer, Target};\n\nfn main() {\n    let target = Target::new(\"frame\");\n    let mut layer = Layer::new(\"sky\");\n    let mut encoder = Encoder::new();\n    println!(\"start\");\n    let mut pass = Target::clear(&target, &mut encoder, 7);\n    layer.draw(&mut pass);\n    layer.rename(\"sea\");\n    pass.draw(1);\n}\n";
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

/// A crate's `JoinHandle` in a crate's `Option` or tuple output, and taken
/// as an input, from the crate's thread and from the program's own: each
/// crosses as rawr's thread handle, joined where the program or the crate
/// joins it (#1274).
#[test]
fn a_crates_join_handle_crosses_nested_and_as_an_input() {
    runs_as_cargo_does(
        "opened_nested",
        "true\ntrue\ntrue true\ntrue\nfalse\ntrue\n",
    );
}

/// A crate's thread that gives a crate's value, alone and in an `Option`:
/// the value becomes rawr's where the program joins the thread (#1275).
#[test]
fn a_crates_join_handle_of_a_crate_value_is_joined() {
    runs_as_cargo_does(
        "paged",
        "3 page 3\nfound page 5\ntrue\nPage { number: 6, text: \"page 6\" }\n",
    );
}

/// A program's thread handle, and a crate's, as the type argument of a
/// crate's generic `Held<T>`, which gives it back to be joined (#1276).
#[test]
fn a_programs_join_handle_fills_a_crates_type_parameter() {
    runs_as_cargo_does("held", "42\ntrue\n");
}

/// A path into a crate none of whose items the program uses, as
/// `opener::number_of` takes a `JoinHandle<Page>`, which a program's
/// thread can't give as a crate's value: the crate is a module, and rawr
/// says why the item can't be used, as it does in a crate with other
/// items. It used to say rustc's E0433 for the crate, as for one the
/// program doesn't depend on (#1267).
#[test]
fn a_crate_with_no_usable_item_says_why_its_item_cant_be_used() {
    let program = "// rawr\n\nfn main() {\n    let _ = opener::number_of(opener::read_in_background(1));\n}\n";
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
            "`number_of` can't be used yet: rawr doesn't run `std::thread::JoinHandle` here in a crate's signature yet"
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
/// (#1278). The generic trait's method is called by its path with the
/// trait's arguments too, as `Counted::<u8>::doubled(x)` (#1282).
#[test]
fn a_provided_methods_opaque_output_whose_supertrait_binds_self() {
    runs_as_cargo_does("doubling", "6\n4\n8\n");
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
/// value passed to `map` (#1317). `use natives::badge::{self, badge};`
/// binds the module and its function of one name, so `badge::Secondary`
/// goes through the module and `badge("..")` calls the function (#1319).
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
///
/// The `Slot` is another crate's, re-exported as a component crate
/// re-exports its parts', with a theme and a renderer that default to
/// that crate's own. Its impls take the widget crate's generic `Text<'a,
/// Theme, Renderer = Plain>` at the crate's own theme, beside the crate's
/// own `Label<'a>`, and the program adds `impl From<Mine> for Slot`. Each
/// child picks its impl by its full type: a `Text` whose theme only the
/// impl gives takes it, so the theme is the crate's, as rustc's impl
/// selection binds it, and a `Label` borrowing a literal opens its frame
/// in the macro's array. A `TextLike` no impl takes is rustc's E0277, with
/// the types the impls take (#1336).
///
/// The program's own `fn check<'a, Theme>() -> Text<'a, Theme>` leaves the
/// theme to the impl its text converts by, in a badge as in a row. In a
/// row whose message type only its last child's impl fixes (`From<Rule>
/// for Slot<'a, ()>`), each child before it still selects the one impl
/// that unifies with its type, as rustc does, rather than E0283 (#1403).
///
/// A `String` takes `From<String>` and a literal `From<&str>`, and only the program's own `Cow<str>` takes `From<Cow<str>>` (#1390).
///
/// A crate macro that wraps its row in `Slot::new(child: impl
/// Compose<Message, T, R> + 'a)` and passes that slot on as `impl
/// Into<Slot<'a, Message>>`, as a component crate's `badge!` does, ties
/// the row's message to the slot's through `impl Compose<Message> for
/// Row<'a, Message>` and the slot's to the badge's through core's
/// reflexive `From<T> for T`, as rustc's impl selection links them, so a
/// later use fixes all three. So does a function whose `C:
/// Compose<Message>` is a where-clause. A badge whose message nothing
/// fixes is rustc's E0282 at its `let` (#1404).
///
/// A view whose message only its declared return type fixes, through each
/// match arm's row `.into()` down to the badges' `Slot::new` and their
/// children's conversions, runs as cargo runs it, as icedxdy's badge
/// previews are written. The slot is also an `Element` by `From<Slot>`, so
/// `impl Into<Slot<'a, Message>>` has an `Into<Element>` impl for a slot
/// beside core's reflexive one, which only the trait's argument rules out.
/// A pill that converts into both a slot and an element has its `into`
/// in a family, whose generic method the runner binds per instance
/// (#1422).
#[test]
fn a_qualified_into_picks_the_crates_from_impl_for_its_argument() {
    runs_as_cargo_does(
        "slots",
        "icon 12 | str Verified | string Verified\nstr into\nstr from | string qualified\n\
         cow borrowed | cow owned\nstr a | str b\nstr lent | str literal\nstr first | str second\n\
         text check 12 | str Verified\nlabel own | string mine 7 | text plain 16\n\
         text into 16 | string mine 3 | label from\n\
         row 8 [string badge secondary 4 [icon 12 | str Verified] | \
         string badge ghost 4 [str Bookmark | icon 12]]\n\
         row 0 [string badge secondary 4 [text check 12 | str Verified]]\n\
         text check 12 | icon 12 | rule\n\
         string badge default 2 [composed row 2 [icon 12 | str Verified]]\n\
         row 8 [string badge secondary 2 [composed row 2 [text check 12 | str Verified]] | \
         string badge ghost 2 [composed row 2 [str Bookmark | icon 12]]]\n\
         string badge default 2 [composed row 2 [str a]] | str b\n\
         composed row 0 [str w]\n\
         row 8 [string pill default 2 [str Badge] | string pill secondary 2 [str Secondary]] | \
         row 8 [string pill secondary 2 [composed row 4 [text check 12 | str Verified]] | \
         string pill ghost 2 [composed row 4 [str Bookmark | icon 12]] | \
         string badge secondary 2 [composed row 2 [text check 12 | str Verified]]] | \
         row 8 [string pill default 2 pressed [composed row 4 [str Open Link | icon 12]]] | \
         row 8 [string badge default 2 [composed row 2 [text check 12 | str Verified]]] Noop\n\
         string badge default 2 [str Verified]\n\
         107997\n",
    );
    let program = "// rawr\nuse natives::compose::{self, Row, Theme};\nuse natives::slots;\n\n\
                   fn main() {\n    let row: Row<'_, ()> = \
                   slots![compose::text_like::<Theme>(\"near\")];\n    \
                   println!(\"{}\", row.draw());\n}\n";
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
    assert!(!stderr.contains("unknown member"), "{stderr}");
    let cargo = cargo_s_report(&stderr);
    for expected in [
        "error[E0277]: the trait bound `natives::compose::Slot<'_, _>: \
         From<TextLike<'_, natives::compose::Theme>>` is not satisfied",
        "the following other types implement trait `From<T>`:",
        "`natives::compose::Slot<'_, Message>` implements `From<&str>`",
    ] {
        assert!(cargo.contains(expected), "{stderr}");
    }
    // A component whose message nothing fixes is rustc's E0282 at its
    // `let`, never a runner whose `_` cargo refuses.
    let program = "// rawr\nuse natives::compose::Slot;\n\nfn main() {\n    \
                   let composed = natives::badge::composed(Slot::from(\"x\"));\n    \
                   let slot = Slot::from(composed);\n    println!(\"{}\", slot.label());\n}\n";
    let root = SHAPES.lease(&[("src/main.rs", program)]);
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
    assert!(!stderr.contains("rawr_adapters"), "{stderr}");
    assert!(
        cargo_s_report(&stderr)
            .contains("error[E0282]: type annotations needed for `Composed<'_, _>`"),
        "{stderr}"
    );
}

/// A literal's `.into()` into a crate's `Slot<'a, M>` that borrows it, in
/// an array whose row alone gives the target, as `Row::with_children([..,
/// "b".into()])` and `wrap(Row::with_children(["w".into()]))`: the literal
/// is lent at `'static`, so no scope frame is needed ahead of the
/// statement, as rustc accepts it (#1328).
#[test]
fn a_literal_into_a_borrowing_slot_a_later_use_types_runs() {
    runs_as_cargo_does(
        "lent",
        "string badge default 2 [composed row 2 [str a]] | str b\ncomposed row 0 [str w]\n\
         str view | str slot\n",
    );
}

/// A crate function whose signature has an array type, as a component
/// crate's `Row::with_children<const N: usize>(children: [Slot<'a, M>; N])`
/// and `fill(&mut [u8; 4], u8)`: an array literal or value crosses by
/// value with `N` taken from its length, `&[T; N]` is lent, `&mut [T; N]`
/// is lent and its writes reach the program's array or the crate struct's
/// field, and `-> [u8; 4]` is Rawr's array (#1330, #1324).
#[test]
fn array_types_cross_as_rustc_has_them() {
    runs_as_cargo_does(
        "arrays",
        "3 children: slot=1 slot=2 slot=3\n2 children: slot=\"a\" slot=\"b\"\n[1, 7, 3, 8]\n[0, 9, 0, 10]\n6 60\n[1, 2, 4, 8] 8\n",
    );
}

/// A crate trait's generic impl for another crate's generic type, as
/// ratzilla's `impl<B: Backend + ..> WebRenderer for Terminal<B>` is, gives
/// `Terminal<DomBackend>` its methods: a `&mut self` one and a `self` one,
/// each taking a script closure (#1450).
#[test]
fn a_crate_traits_generic_impl_gives_a_generic_crate_type_its_methods() {
    runs_as_cargo_does("renderer", "page | hi!\n");
}

/// `?` turns a crate's error into an `io::Error` by the crate's `impl
/// From<Error> for io::Error`, and into a `Box<dyn Error>` by the
/// `std::error::Error` impl thiserror's derive writes, which a `Box` of the
/// error is too (#1451).
#[test]
fn a_crate_errors_from_impl_and_derived_error_impl_serve_question_mark() {
    runs_as_cargo_does(
        "errors",
        "true\nUnable to retrieve element: \nUnable to retrieve element: \n\
         Unable to retrieve element: x\n",
    );
}

/// A universe runner (`RAWR_UNIVERSE_RUNNER=1`, as ?play's image builds
/// one) loads a marked file in a `rawr::universe::Universe` of the bindings
/// its build generated for the crate's dependencies, and runs it in
/// process: the file calls into the crate's `natives` and prints what
/// `cargo run` of the same file prints (#1387).
#[test]
fn a_universe_runner_loads_a_marked_file_against_the_crates_bindings() {
    let program = SHAPES.read("mains/universe.rs");
    let root = SHAPES.lease(&[("src/main.rs", &program)]);
    let expected = "printed by the universe\n9\n";
    let native = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&*root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(native.status.success(), "{}", text(&native.stderr));
    assert_eq!(text(&native.stdout), expected);

    let directory = crate::support::scratch();
    let out = directory.join("universe");
    let built = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env("RAWR_UNIVERSE_RUNNER", "1")
        .env("RAWR_UNIVERSE_OUT", &out)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", text(&built.stderr));
    assert_eq!(text(&built.stdout), expected, "{}", text(&built.stderr));

    // The runner is where its caller asked for it (#1752).
    let runner = out.join("runner");
    assert!(runner.is_file(), "no runner in {}", out.display());
    // It serves the file as ?play's server serves a snippet.
    let socket = std::env::temp_dir().join(format!("rawr-1387-{}.sock", std::process::id()));
    let mut server = Command::new(&runner)
        .env_clear()
        .env("RAWR_PLAY_ROOT", &*root)
        .env(rawr::play::SOCKET, &socket)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{}: {error}", runner.display()));
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(server.stdout.take().unwrap()),
        &mut ready,
    )
    .unwrap();
    assert_eq!(ready.trim_end(), rawr::play::READY);
    let ran = rawr::play::run(
        &socket,
        &rawr::play::Request {
            source: program,
            env: Vec::new(),
            time: std::time::Duration::from_secs(60),
            deadline: None,
            memory: None,
        },
    );
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_file(&socket);
    let ran = ran.unwrap();
    assert_eq!(ran.status, 0, "{}", text(&ran.stderr));
    assert_eq!(text(&ran.stdout), expected, "{}", text(&ran.stderr));
}
