//! A script's own types and functions cross into a native generic entry
//! point in the shape of a GUI library's `run` (D594): `run::<S, M, U, V>`
//! makes the state with the script type's `Default`, names it by
//! `type_name`, and calls the script's `update(&mut S, M)` and its
//! higher-ranked `view(&S) -> Widget<'_, M>`, whose label borrows the
//! state's string. The output is `cargo run`'s, and a body edit takes
//! effect at the next event with the state kept. Build the driver first,
//! as for `plain`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::support::fixture::Lease;
use crate::support::{cargo_s_report, start, Fixture};

/// The fixture, whose lanes hold the program's own native modules, which
/// only some variants declare, beside them, so that rustfmt finds them
/// from each.
static CROSSING: Fixture = Fixture::with(
    "crossing",
    &[
        ("src/download.rs", "variants/download.rs"),
        ("src/flagged.rs", "variants/flagged.rs"),
    ],
);

/// A lane of the fixture, with `variant` from `variants/` as its `main.rs`
/// when given.
fn lane(variant: Option<&str>) -> Lease {
    match variant {
        Some(variant) => {
            let main = CROSSING.read(&format!("variants/{variant}"));
            CROSSING.lease(&[("src/main.rs", &main)])
        }
        None => CROSSING.lease(&[]),
    }
}

/// `rawr-import run` on a lane of the fixture with `variant` from
/// `variants/` as its `main.rs` when given: its stdout without the process
/// id, which `cargo run` of the same crate prints too.
fn printed(variant: Option<&str>) -> String {
    printed_in(&lane(variant))
}

/// `rawr-import run` on the project at `root`, as `printed` gives it.
fn printed_in(root: &std::path::Path) -> String {
    let directory = crate::support::scratch();
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .env_remove("EVENTS_COUNT")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
    stdout
        .lines()
        .filter(|line| !line.starts_with("pid "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What both programs print after their first lines, as `cargo run` does.
const EVENTS: &str = "1 view \n1 message Increment\n2 view count 1\n2 message Increment\n\
                      3 view count 2\n3 message Increment\nlast view count 3";

#[test]
fn a_script_state_message_update_and_view_run_under_a_native_generic_run() {
    assert_eq!(printed(None), format!("title crossing::Counter\n{EVENTS}"));
}

/// iced's shape: `application(Counter::update, Counter::view)` takes the
/// methods through traits implemented for every `Fn` of their shape, and
/// the theme and renderer are the ones the view's widget names.
#[test]
fn methods_cross_through_traits_over_fn_as_iced_takes_them() {
    assert_eq!(
        printed(Some("application.rs")),
        format!("title crossing::Counter\ntheme events::Theme\n{EVENTS}")
    );
}

/// iced's counter: `column![button("Increment"), text(value).size(50),
/// ..]` from `fn view(&self) -> impl Widget<Message>`. Only `Widget`'s
/// defaulted `Renderer`, a parameterless alias that is the only name of
/// its type, and the crate's impls decide the hidden type's renderer
/// (D604), and a string literal fills `button`'s content as `&'static
/// str`. The output is `cargo run`'s.
#[test]
fn a_counter_whose_view_only_a_traits_defaults_and_impls_fix_runs() {
    assert_eq!(
        printed(Some("counter.rs")),
        "title crossing::Counter\n\
         renderer events::widgets::fallback::Renderer<events::widgets::backend::Gpu, events::widgets::backend::Soft>\n\
         1 view pad 20 [Increment] 0@50 [Decrement]\n1 message Increment\n\
         2 view pad 20 [Increment] 1@50 [Decrement]\n2 message Increment\n\
         3 view pad 20 [Increment] 2@50 [Decrement]\n3 message Increment\n\
         4 view pad 20 [Increment] 3@50 [Decrement]\n4 message Decrement\n\
         last view pad 20 [Increment] 2@50 [Decrement]"
    );
}

/// iced's counter with `Button<'a, Message, W, Theme>`, whose `Widget`
/// impl holds when its content's does: the content is a string literal,
/// whose impl for `&str` no facade header writes. The impl is chosen by
/// its self type, as rustc chooses it, so the tile's theme is the one the
/// view's `impl Widget<Message>` gives, and the tile lives in the view's
/// region at that type. The output is `cargo run`'s.
#[test]
fn a_widget_whose_impl_bounds_its_contents_takes_the_views_theme() {
    let (rawr, cargo) = both_printed("tiled.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 20 <'Increment'> 0@50 <'Decrement'>\n1 message Increment"),
        "{rawr}"
    );
}

/// A view of text alone in iced's earlier widgets, `fn view(&self) ->
/// Element<'_, Message>`, whose element is made by `Element::from` and by
/// `.into()`: only the return type names the message, theme and renderer,
/// as rustc infers them from it, and each value lives in the view's region
/// at its full type. The output is `cargo run`'s.
#[test]
fn a_text_only_view_takes_its_types_from_its_return_type() {
    for variant in ["classic.rs", "classic_into.rs"] {
        let (rawr, cargo) = both_printed(variant, "");
        assert_eq!(rawr, cargo, "{variant}");
        assert!(rawr.contains("1 view center hi"), "{variant}: {rawr}");
    }
}

/// iced's `.boxed()`: a method of an extension trait no widget implements
/// itself, which a blanket impl over the crate's own `Widget` gives every
/// widget, on a button and on text of generic types. The output is `cargo
/// run`'s.
#[test]
fn a_method_of_a_blanket_extension_trait_runs_on_generic_widgets() {
    let (rawr, cargo) = both_printed("framed.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 20 [Increment] [Decrement]\n1 message Increment")
            && rawr.contains("3 view pad 20 [Increment] 2@50\n3 message Increment")
            && rawr.ends_with("last view pad 20 [Increment] 3@50"),
        "{rawr}"
    );
}

/// iced's modal and toast helpers: an `impl Widget<Message>` parameter
/// called with a button and with text, and a function generic over its
/// message that builds a crate widget, each compiled once per
/// instantiation its callers use (D629). The output is `cargo run`'s.
#[test]
fn generic_functions_over_crate_types_run_once_per_instantiation() {
    let (rawr, cargo) = both_printed("instances.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 pad 5 [Increment] pad 7 0@50 [Decrement]")
            && rawr.ends_with("last view pad 0 pad 5 [Increment] pad 7 2@50 [Decrement]"),
        "{rawr}"
    );
}

/// iced's text, system_information and multi_window: `new` gives the
/// state and its first task, `(Self, Task<Message>)`, which
/// `application(..)` takes through `BootFn` and the tuple's `IntoBoot`.
/// The task's message arrives first, and `update` calls `Self::new()`
/// again. The output is `cargo run`'s.
#[test]
fn a_boot_function_giving_the_state_and_a_task_runs() {
    let (rawr, cargo) = both_printed("boot.rs", "Refresh");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("boot message Loaded(7)\n1 view pad 0 <x Refresh:0> \"7\"")
            && rawr.contains("1 message Refresh(false)\ntask message Loaded(7)"),
        "{rawr}"
    );
}

/// iced's delineate: `selector::find(id)` is generic over `Selector`, a
/// trait with an associated output that the crate also implements for
/// `FnMut` closures, and takes the widget `Id` with no closure. Its task's
/// `Option<Target>` maps into a message. iced's scrollable: a palette
/// field's `Color`, which the program names by no path, converts into a
/// `Background` by `.into()` through `From`. The output is `cargo run`'s.
#[test]
fn a_selector_finds_by_id_and_a_fields_color_converts_into_a_background() {
    let (rawr, cargo) = both_printed("found.rs", "Refresh");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("Some(50.0) Color(Color { r: 3, g: 4, b: 5 })"),
        "{rawr}"
    );
}

/// iced's system_information: `Task::map(Message::InformationReceived)`
/// calls the variant on another thread, as iced's executor does, with a
/// crate value native code made. The message is made where it is
/// delivered, and the view reads the crate value's fields from a variant.
/// The output is `cargo run`'s.
#[test]
fn a_crate_value_mapped_on_another_thread_reaches_its_message() {
    let (rawr, cargo) = both_printed("system.rs", "Refresh");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "boot message InformationReceived(Info { name: \"fixture\", cores: Some(4) })"
        ) && rawr.ends_with("last view pad 0 <Refresh:0> \"fixture with 4 cores\""),
        "{rawr}"
    );
}

/// iced's text: a `Task<Result<Vec<Family>, Error>>` of a crate enum is
/// mapped on another thread by `Result::ok`, which gives back a crate
/// value through an arena of the call's own, and then by a variant, whose
/// message is made where it is delivered. `and_then` takes the bounded
/// generic `Task::done`, `listen_with` takes a closure where it takes a
/// function pointer and calls it on another thread with crate values, and
/// `pick(..)` takes a slice of the crate enum with
/// `Family::to_string`, which borrows each option. The output is `cargo
/// run`'s.
#[test]
fn text_example_task_chains_pointer_callables_and_crate_slices_run() {
    let (rawr, cargo) = both_printed("fonts.rs", "Mono");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("boot message FamiliesListed([Monospace, Serif])\nheard Rescaled(2.0)")
            && rawr.ends_with("last view pad 0 <Mono: *Mono|Serif> \"Mono at 2 in 7\""),
        "{rawr}"
    );
}

/// Generic methods over a crate's trait, a generic associated function
/// giving `impl Widget<M>`, and a generic function whose parameter reaches
/// a crate function only in its body, called with a crate color and with a
/// script type that converts into one: each is compiled once per
/// instantiation its callers use (D629). The output is `cargo run`'s.
#[test]
fn generic_methods_and_body_only_generics_run_once_per_instantiation() {
    let (rawr, cargo) = both_printed("methods.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "1 view pad 0 pad 5 [Increment] pad 7 0@50 pad 1 [Decrement] \
             painted rgb(1, 2, 0)@16 painted rgb(10, 20, 30)@16"
        ) && rawr.contains("4 message Decrement"),
        "{rawr}"
    );
}

/// iced's custom_quad and custom_widget: the program's own widget
/// implements the crate's widget trait over any message, theme and
/// renderer its bound on the crate's renderer trait admits, and `draw`
/// calls the renderer's `fill_quad`. The impl is compiled for the use the
/// crate's bound makes of it, at the crate's renderer (D629), and its
/// `press` gives an `Option` of the impl's own parameter. The output is
/// `cargo run`'s.
#[test]
fn a_script_impl_generic_over_a_crate_bound_is_compiled_per_use() {
    let (rawr, cargo) = both_printed("quad.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 custom 10 [quad 10r2 rgb(1, 2, 3)] [Increment] 0@16")
            && rawr
                .ends_with("last view pad 0 custom 13 [quad 13r2 rgb(1, 2, 3)] [Increment] 3@16"),
        "{rawr}"
    );
}

/// iced's the_matrix: a script impl of a crate trait whose associated
/// `State` is a std type Rust code holds as a script value, `RefCell<Vec<
/// i64>>`, which the trait asks `Default` of. The runner names it by a
/// proxy whose `Default` is the type's, and `draw` changes the state the
/// crate keeps between its calls. The output is `cargo run`'s.
#[test]
fn an_associated_state_held_as_a_script_value_crosses() {
    let (rawr, cargo) = both_printed("state.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view canvas[primary 3 cells [1, 10, 20] at 40x30 tick 0#1]")
            && rawr.contains(
                "pressed Increment: canvas[primary 3 cells [2, 10, 20] at 40x30 tick 0#1]"
            ),
        "{rawr}"
    );
}

/// iced's bezier_tool (#990): a script `update` that sets the `&mut
/// Option<Pending>` state the crate lends it, where `Pending` is the
/// program's own type. The crate keeps what the update left, as rustc's
/// does, and `draw` shows it. The output is `cargo run`'s.
#[test]
fn a_state_a_script_update_sets_is_kept_by_the_crate() {
    let (rawr, cargo) = both_printed("pending.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("pressed Increment: canvas[primary pending Some(One { from: 9 })#1]"),
        "{rawr}"
    );
}

/// iced's component: a generic script type implements a crate trait over
/// the message its boxed callback makes, `view` gives `impl Widget<
/// Message>` and `fallback` an `Option<Message>`, both of the impl's own
/// parameter. The impl is compiled for the use the crate's bound makes of
/// it (D629), the view's widgets are made in a region anchored on `self`,
/// and the callback crosses inside the script value. The output is `cargo
/// run`'s.
#[test]
fn a_generic_script_impl_with_an_opaque_view_runs() {
    let (rawr, cargo) = both_printed("panel.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view panel pad 0 [Increment] 0@16\n1 message Changed(1)")
            && rawr.contains("4 message Changed(0)")
            && rawr.ends_with("last view panel pad 0 [Increment] 0@16"),
        "{rawr}"
    );
}

/// iced's multi_window: a view passes a window's view, a method whose
/// `impl Widget` output is declared after its caller, to a generic crate
/// function. The caller waits for that output, and a fresh build asks the
/// crate for what the window's view names before reporting the caller's
/// failure. The output is `cargo run`'s.
#[test]
fn a_view_passing_a_later_methods_opaque_output_to_the_crate_runs() {
    let (rawr, cargo) = both_printed("windows.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 3 pad 0 One 1 0@16 [Increment]\n1 message Increment")
            && rawr.ends_with("last view pad 3 pad 0 One 1 3@16 [Increment]"),
        "{rawr}"
    );
}

/// `rawr-import run` and `cargo run` of a lane of the fixture with
/// `variant` as its `main.rs`, pressing the labels of `presses` in turn:
/// both programs' stdout without the process id.
fn both_printed(variant: &str, presses: &str) -> (String, String) {
    both_printed_in(&lane(Some(variant)), presses, &[])
}

/// `rawr-import run` and `cargo run` of the project at `root` with
/// `flags`, as `both_printed` gives them.
fn both_printed_in(root: &std::path::Path, presses: &str, flags: &[&str]) -> (String, String) {
    let directory = crate::support::scratch();
    let output = |mut command: Command| {
        let run = command
            .current_dir(root)
            .env("EVENTS_PRESS", presses)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("RUSTFLAGS")
            .env_remove("RAWR_VERBOSE")
            .env_remove("EVENTS_COUNT")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(run.status.success(), "stdout:\n{stdout}\nstderr:\n{stderr}");
        // Without the process id, which differs between the two.
        stdout
            .lines()
            .filter(|line| !line.starts_with("pid "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut rawr = Command::new(env!("CARGO_BIN_EXE_rawr-import"));
    rawr.arg("run")
        .arg(root)
        .args(flags)
        .env("RAWR_HOME", directory.join("rawr-home"));
    let mut cargo = Command::new("cargo");
    cargo.args(["run", "-q"]).args(flags);
    (output(rawr), output(cargo))
}

/// A package whose `default` feature enables `x` (#1268): marked code's
/// `cfg!(feature = "x")` and `#[cfg(feature = "x")]` statement, and a
/// native module's `cfg!`, hold as Cargo enables the features, with the
/// default ones and with `--no-default-features`. The output is `cargo
/// run`'s.
#[test]
fn a_crate_s_features_follow_cargo_s_feature_flags() {
    let manifest = CROSSING.read("Cargo.toml").replace(
        "[dependencies]",
        "[features]\ndefault = [\"x\"]\nx = []\n\n[dependencies]",
    );
    let main = CROSSING.read("variants/featured.rs");
    let root = CROSSING.lease(&[("Cargo.toml", &manifest), ("src/main.rs", &main)]);
    for (flags, x) in [(&[][..], true), (&["--no-default-features"][..], false)] {
        let (rawr, cargo) = both_printed_in(&root, "", flags);
        assert_eq!(rawr, cargo, "{flags:?}");
        let only = if x { "only with x" } else { "only without x" };
        assert_eq!(
            rawr,
            format!("x {x}\n{only}\nnative x {x}\nsame 3"),
            "{flags:?}"
        );
    }
}

/// iced's `window::Settings { #[cfg(target_os = "macos")]
/// platform_specific: .., ..Default::default() }` (#1331): a crate struct
/// built with a field under `#[cfg(..)]`, and an arm and an array element
/// under one, follow the options rustc sets for the target Cargo builds
/// for. The output is `cargo run`'s.
#[test]
fn a_crate_struct_built_with_a_cfg_field_follows_the_target() {
    let (rawr, cargo) = both_printed("configured.rs", "");
    assert_eq!(rawr, cargo);
    let (radius, family) = if cfg!(unix) {
        (4, "unix")
    } else {
        (5, "windows")
    };
    let wide = cfg!(target_pointer_width = "64");
    assert_eq!(rawr, format!("quad 3 {radius}\nsizes 2\n{family} {wide}"));
}

/// iced's tour (#1074): `padded_button("Back")`, a generic function of the
/// program's own compiled per instantiation, takes the message that a
/// later `.on_press(..)` fixes, as rustc infers it. The output is `cargo
/// run`'s.
#[test]
fn a_generic_functions_parameter_a_later_use_fixes_runs() {
    let (rawr, cargo) = both_printed("padded.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("message NextPressed") && rawr.contains("message BackPressed"),
        "{rawr}"
    );
}

/// iced's gallery and ferris (#1099, #1071): `picture(&handle)` and
/// `picture("ferris")` take the handle the renderer's associated type
/// fixes through the widget's impl, not the argument's own type. The
/// output is `cargo run`'s.
#[test]
fn a_parameter_an_impls_associated_type_fixes_runs() {
    let (rawr, cargo) = both_printed("pictured.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("picture Handle { name: \"sun\" }@1")
            && rawr.contains("explain #ffffff picture Handle { name: \"ferris\" }@2"),
        "{rawr}"
    );
}

/// iced's screenshot (#1632): `picture(handle)` of a `&Handle` a pattern
/// binds, boxed by a method call in one arm of an `if`, takes the handle
/// that the renderer the element's later use fixes gives through the
/// widget impl's `Renderer: imaging::Renderer<Handle = Handle>`, not the
/// argument's own `&Handle` (E0271 before). The output is `cargo run`'s.
#[test]
fn a_handle_a_later_renderer_fixes_through_a_boxed_method_runs() {
    let (rawr, cargo) = both_printed("screened.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 Press the button@16 [Increment]")
            && rawr.contains("2 view pad 0 picture Handle { name: \"shot\" }@1 [Increment]"),
        "{rawr}"
    );
}

/// iced's editor (#1633): `action(new_icon(), ..)` calls the program's
/// `fn new_icon<Message>() -> impl Widget<Message>`, whose `Message` only
/// the bound of `action`'s parameter fixes, from the message a later
/// argument gives, as rustc infers it. The output is `cargo run`'s.
#[test]
fn a_generic_functions_parameter_the_expected_bound_fixes_runs() {
    let (rawr, cargo) = both_printed("iconed.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 message Increment")
            && rawr.contains("3 view pad 0 pad 0 <'Increment'> New@16 pad 0 <S@12> Save@16 2@16"),
        "{rawr}"
    );
}

/// iced's gallery (#1213): `tile(picture(handle))` given by a function
/// written `-> impl Widget<Message> + 'a`, and by one written `->
/// Option<impl Widget<Message>>`, where only the output's defaulted
/// renderer fixes the picture's handle. The output is `cargo run`'s.
#[test]
fn a_handle_only_an_opaque_outputs_renderer_fixes_runs() {
    let (rawr, cargo) = both_printed("floated.rs", "Increment");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 <picture Handle { name: \"sun\" }@0> none [Increment]")
            && rawr.contains(
                "2 view pad 0 <picture Handle { name: \"sun\" }@1> <picture Handle { name: \"sun\" }@0>"
            ),
        "{rawr}"
    );
}

/// iced's checkbox: `application(State::default, ..)` gives an opaque
/// `impl Program` that `fonts([..])` and `run()` take, `on_toggle_maybe`
/// takes an `Option` of a variant's constructor, and `style` takes a crate
/// function of two inputs. A press toggles; the second toggle only takes
/// presses once the first is checked. The output is `cargo run`'s.
#[test]
fn a_checkbox_with_an_optional_callable_and_a_crate_style_runs() {
    let (rawr, cargo) = both_printed("checkbox.rs", "Styled,Default,Styled");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("2 message DefaultToggled(true)")
            && rawr.contains("3 message StyledToggled(true)")
            && rawr.contains("last view pad 0 <x Default:0> <x Styled:1>"),
        "{rawr}"
    );
}

/// iced's slider: `logo(..)` gives an opaque `impl Widget` that `column!`
/// boxes through `Widget::_boxed`, and `default(50)` takes the literal as
/// the slider's `u8`. `caption(self.value)` calls the function where a
/// macro has the same name, as iced's `text(self.value)` does, and its
/// type's only name is an alias that passes its parameters on. The output
/// is `cargo run`'s.
#[test]
fn a_slider_beside_an_opaque_widget_runs() {
    let (rawr, cargo) = both_printed("slider.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 1..=100@50/Some(50) \"50\" logo50@16")
            && rawr.contains("1 message SliderChanged(100)")
            && rawr.contains("last view pad 0 1..=100@100/Some(50) \"100\" logo100@16"),
        "{rawr}"
    );
}

/// #886: `badge(..)` gives an opaque `impl Framed<Message, Theme,
/// Renderer>` that nothing at the call fixes, and `opaque(slider(..))` one
/// whose theme and renderer it doesn't fix. `.map(..)` fixes the message,
/// and `column!`'s boxing of the `Map` fixes the theme and renderer through
/// `Map`'s impl, whose where-clause asks the opaque type for `Widget`, a
/// supertrait of `Framed`. The call that made the value takes the types
/// that later use fixes, held in a local or not, as rustc infers them. Each
/// opaque widget's boxed element stays in the view's region, so a column of
/// them alone is the view's widget. The output is `cargo run`'s.
#[test]
fn an_opaque_widget_takes_the_types_a_later_use_fixes() {
    let (rawr, cargo) = both_printed("deferred.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "1 view pad 0 opaque 1..=100@50/None map badge50@16 map badge50@16 badge50@16"
        ) && rawr.contains(
            "last view pad 0 opaque 1..=100@100/None map badge100@16 map badge100@16 badge100@16"
        ),
        "{rawr}"
    );
}

/// iced's ferris (#887): `f32::from(self.angle)` and `angle.into()` call
/// the crate's `impl From<Turn> for f32`, its impl of a std trait for a std
/// type. The output is `cargo run`'s.
#[test]
fn a_crates_impl_of_a_std_trait_for_a_std_type_is_callable() {
    let (rawr, cargo) = both_printed("degrees.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 Turn(0.0)..=Turn(1.0)@Turn(0.25)/None \"90\" \"45\" \"0\"")
            && rawr.contains(
                "last view pad 0 Turn(0.0)..=Turn(1.0)@Turn(1.0)/None \"360\" \"180\" \"360\""
            ),
        "{rawr}"
    );
}

/// iced's ferris: `f32::from(self.rotation.degrees())` converts a method's
/// output through the crate's `impl From<Turn> for f32`, as the field form
/// does, and so does `.into()` of a method's output. The output is `cargo
/// run`'s.
#[test]
fn a_crates_from_impl_converts_a_methods_output() {
    let (rawr, cargo) = both_printed("spun.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 Turn(0.0)..=Turn(1.0)@Turn(0.25)/None \"Rotation: 90\" \"45\"")
            && rawr.contains(
                "last view pad 0 Turn(0.0)..=Turn(1.0)@Turn(1.0)/None \"Rotation: 360\" \"180\""
            ),
        "{rawr}"
    );
}

/// A crate trait's own `into` out of scope leaves `gauge.into()` the
/// `Into::into` of the crate's `From<Gauge>` impls, which the target picks
/// (D741). The output is `cargo run`'s.
#[test]
#[ignore = "blocked: the Application path can't yet prove that only `Into::into` is in scope and leave its instance to the target (`gauge.into()` beside an unimported crate trait's `into`)"]
fn a_crate_traits_own_into_out_of_scope_leaves_into_to_the_target() {
    let (rawr, cargo) = both_printed("gauged.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.contains("150 15"), "{rawr}");
}

/// A crate trait's own `into` in scope beside `Into::into` is rustc's
/// E0034: the target never chooses between two traits' methods (D741).
#[test]
fn a_crate_traits_own_into_in_scope_is_ambiguous_with_into() {
    let stderr = rejected_as_cargo("gauged_halved.rs", "E0034");
    assert!(
        stderr.contains("multiple applicable items in scope"),
        "{stderr}"
    );
}

/// `let y = turn.into(); y * 2.0` fixes only that `y` is a float, and the
/// crate converts a `Turn` to `f32` and to `f64`: rustc's E0283 at `y`,
/// never the first impl that fits (D741).
#[test]
fn a_target_known_only_as_a_float_is_ambiguous_between_from_impls() {
    let stderr = rejected_as_cargo("turned_open.rs", "E0283");
    assert!(
        stderr.contains("type annotations needed") && stderr.contains("main.rs:10:9"),
        "{stderr}"
    );
}

/// `let _unused = turn.into();` with nothing fixing the target is rustc's
/// E0283 (D741).
#[test]
fn a_target_no_use_fixes_is_ambiguous_between_from_impls() {
    let stderr = rejected_as_cargo("turned_unfixed.rs", "E0283");
    assert!(
        stderr.contains("type annotations needed") && stderr.contains("main.rs:9:9"),
        "{stderr}"
    );
}

/// iced's styling: `primary as fn(&Theme, _) -> _` casts a crate function
/// to a `fn` pointer whose parts inference fills, and the array's next
/// crate function coerces to that pointer, so the types agree with
/// rustc's and `rawr-import check` passes.
#[test]
fn crate_functions_cast_to_an_inferred_fn_pointer_share_an_array() {
    passes_check("styles.rs");
}

/// iced's styling, run: each toggle takes its style from the array. The
/// output is `cargo run`'s.
#[test]
fn crate_functions_in_a_fn_pointer_array_style_their_toggles() {
    let (rawr, cargo) = both_printed("styles.rs", "Primary");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 pad 0 <Primary:1> <Secondary:3>")
            && rawr.contains("last view pad 0 pad 0 <x Primary:1> <x Secondary:3>"),
        "{rawr}"
    );
}

/// iced's vectorial_text: a closure's parameter that only a slider's
/// `RangeInclusive<T>` types takes `T` from the closure's `fn(f32) -> _`,
/// so the call's `2.0..=80.0` is a range of `f32`, as rustc infers it, and
/// `rawr-import check` passes.
#[test]
fn a_closures_range_parameter_takes_its_element_from_the_other_arguments() {
    passes_check("labeled.rs");
}

/// iced's vectorial_text, run: the closure passes its `fn(f32) -> _`
/// parameter on to the slider. The output is `cargo run`'s.
#[test]
fn a_closure_passes_its_range_and_fn_pointer_to_a_slider() {
    let (rawr, cargo) = both_printed("labeled.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 pad 0 \"Size\" 2.0..=80.0@40.0/None")
            && rawr.contains("1 message SizeChanged(80.0)"),
        "{rawr}"
    );
}

/// iced's gradient: `slider(Radians::RANGE, ..)` reads an associated
/// constant whose type is std's `RangeInclusive` of the crate's own type,
/// and the slider takes it at that type. The output is `cargo run`'s.
#[test]
fn a_range_of_a_crate_type_from_an_associated_constant_crosses() {
    let (rawr, cargo) = both_printed("turned.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 Turn(0.0)..=Turn(1.0)@Turn(0.25)/None")
            && rawr.contains("1 message Turned(Turn(1.0))")
            && rawr.contains("last view pad 0 Turn(0.0)..=Turn(1.0)@Turn(1.0)/None"),
        "{rawr}"
    );
}

/// iced's qr_code, solar_system and screenshot: `column!` boxes an
/// `Option` of a slider or of a caption through the crate's impl of its
/// widget trait for `Option<T>`, a std type, as it boxes the crate's own
/// widgets. The output is `cargo run`'s.
#[test]
fn an_option_of_a_widget_boxes_through_the_crates_impl_for_option() {
    let (rawr, cargo) = both_printed("optional.rs", "slide,slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 1..=100@50/None none none")
            && rawr.contains("last view pad 0 1..=100@100/None \"100\" 1..=100@100/None"),
        "{rawr}"
    );
}

/// iced's websocket and screenshot: `caption`, a generic crate function
/// whose name a macro and a module share, is passed as a value to
/// `Iterator::map` over `&String` and to `Option::map` over `u8`. The
/// closure's input fixes its `impl ToString`, and the view's widget fixes
/// its theme, as rustc infers them. The output is `cargo run`'s.
#[test]
fn a_generic_crate_function_named_as_a_value_maps_as_rustc_infers_it() {
    let (rawr, cargo) = both_printed("valued.rs", "slide,slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 1..=100@50/None pad 0 \"first\" \"second\" none")
            && rawr.ends_with("\"at 100\" \"at 100\" \"100\""),
        "{rawr}"
    );
}

/// iced's styling and delineate: crate constants that aren't scalars, a
/// struct (`Font::MONOSPACE`) and a `&'static [Self]` (`Theme::ALL`), are
/// read by their paths, and a macro's `$crate::Alias::new(..)` reaches the
/// associated function of a type whose only name is that alias. The output
/// is `cargo run`'s.
#[test]
fn constants_of_no_scalar_and_a_sole_alias_run() {
    let (rawr, cargo) = both_printed("styling.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("last view pad 0 \"2 looks\"/mono0 \"mono0\" \"0\"/plain0"),
        "{rawr}"
    );
}

#[test]
fn methods_of_a_generic_target_are_callable_through_its_opaque_alias() {
    let (rawr, cargo) = both_printed("alias.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "quad 10r2 rgb(1, 2, 3), quad 4r1 rgb(4, 5, 6) at 3");
}

/// A provided method reaches the script's `labels`, and what it pushes
/// onto the `&mut Vec<String>` native code lends it is in native code's
/// vector after the callback returns (#365). The output is `cargo run`'s.
#[test]
fn a_provided_method_reaches_a_script_impl_through_a_blanket_trait() {
    let (rawr, cargo) = both_printed("provided.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr.matches("script widget").count(), 5, "{rawr}");
    assert!(rawr.contains("last view pad 0 script widget"), "{rawr}");
}

/// A widget whose only name is an alias, the fixture's `Caption`, sits in
/// one `column!` beside another widget of the same shape (#339). The
/// output is `cargo run`'s.
#[test]
fn a_sole_alias_widget_sits_beside_a_widget_of_its_shape() {
    let (rawr, cargo) = both_printed("caption_beside.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("last view pad 0 \"0\"/mono0 'beside'@12 \"last\""),
        "{rawr}"
    );
}

/// A crate struct's `String` field is read in place, as Rust reads it:
/// `format!`, a method, `&` and `==` leave the struct whole, and a
/// by-value use moves only that field. The output is `cargo run`'s.
#[test]
fn a_crate_structs_string_field_is_read_in_place() {
    let (rawr, cargo) = both_printed("caption.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "[quartz] Pixels(16.0) 6 quartz true\nquartz Pixels(16.0) quartz"
    );
}

/// A `&self` method's `&[T]`, as iced's `combo_box::State::options`, is
/// lent in place for as long as the receiver's borrow lasts (#687), for `T`
/// a `String` and a crate struct: `.len()`, indexing, a `for` loop and
/// `.iter().position(..)` read the native elements, and the struct's clone
/// counter shows none was copied. The output is `cargo run`'s.
#[test]
fn a_slice_a_method_lends_from_its_receiver_is_read_in_place() {
    let (rawr, cargo) = both_printed("options.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.ends_with("heavy at Some(1), 0 clones"), "{rawr}");
    // The vectors move their families into `Choices::new`, as Rust moves
    // them (#698).
    assert!(rawr.contains("0 clones making the choices"), "{rawr}");
    assert!(
        rawr.contains("1 options, 0 clones moving a local"),
        "{rawr}"
    );
}

/// A crate call's `&mut String` and `&mut Vec<u8>` are the program's own,
/// lent in place (#716): the room `reserve` makes is still there at the
/// next call, as rustc's borrow keeps the one allocation, and the program
/// reads and changes what the calls wrote between them. A trait the crate
/// implements for `&str` of any lifetime takes the program's strings at the
/// call's lifetime (#717). The output is `cargo run`'s.
#[test]
fn mutable_buffers_are_lent_in_place() {
    let (rawr, cargo) = both_printed("buffers.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.contains("room still kept true"), "{rawr}");
    assert!(rawr.contains("[1] 1, room kept true"), "{rawr}");
    assert!(rawr.contains("[Basalt] is 6 wide"), "{rawr}");
}

/// The `&mut Pixel` a crate method returns, inherent or of a trait as
/// image's `GenericImage::get_pixel_mut`, is a place (#1318): `.0 = [..]`,
/// `*p = Pixel(..)`, `.0[3] += 1`, the same through a local that keeps it,
/// and the reference lent on to another crate call write the crate's own
/// pixel, and the crate reads them back. The method is called once per
/// statement or binding, as rustc holds the reference it gives (#1325):
/// the crate counts 13 calls, as under `cargo run`. So is an `Option<&mut
/// Pixel>` a getter gives, as pane_grid's `state.get_mut(pane)`, tested and
/// written through one call (#1334), and the `&mut` and `Option<&mut>` of
/// a strip a scope frame's region holds, as quick-xml's
/// `reader.config_mut()` (#1333): the strip counts 4 calls. The output is
/// `cargo run`'s. The reference pins the buffer while it lives: reading
/// the buffer before its last use is rustc's E0502.
#[test]
fn a_mutable_reference_a_crate_method_returns_is_a_place() {
    let (rawr, cargo) = both_printed("painted.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.starts_with("Pixel([5, 6, 7, 9])"), "{rawr}");
    assert!(
        rawr.contains(
            "strip [Pixel([2, 0, 0, 0]), Pixel([0, 5, 6, 0]), Pixel([0, 0, 0, 21])] lent 4"
        ),
        "{rawr}"
    );
    assert!(rawr.ends_with("lent 13 times"), "{rawr}");
    let stderr = rejected_as_cargo("painted_pinned.rs", "E0502");
    assert!(stderr.contains("main.rs:7:"), "{stderr}");
}

#[test]
fn generic_struct_fields_and_a_borrowed_field_store_match_rust() {
    let (rawr, cargo) = both_printed("fields.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Shrink/Shrink\n13\n7\n7\n7\n13x14");
}

/// iced's tooltip: a derived `Default` builds a field of a crate enum by the
/// crate's own `Default`, a `match` reads it, and a box of a string hint
/// takes a crate function `fn(&Theme) -> Style` as its style. The output
/// is `cargo run`'s.
#[test]
fn a_tooltip_with_a_crate_default_field_and_a_crate_style_runs() {
    let (rawr, cargo) = both_printed("tooltip.rs", "Delay,Delay");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 [Top:9] <Delay:0>")
            && rawr.contains("2 view pad 0 [Bottom:9] <x Delay:0>")
            && rawr.contains("last view pad 0 [Top:9] <Delay:0>"),
        "{rawr}"
    );
}

/// iced's `text!`: a macro's `$crate::Alias::new(format!(..))`, where the
/// alias passes its parameters on to a type, reaches the type's associated
/// function, as a `text` function of the same name sits beside it, and a
/// lazy iterator fills a column's `impl IntoIterator`, over a range as
/// iced's text example's `column(sizes.map(..))` and over the state as its
/// events example's `column(self.last.iter().map(..))`. The text is
/// invariant in its lifetime, as iced's is, so each is made in the view's
/// region and none at `'static`. The output is `cargo run`'s.
#[test]
fn a_macro_reaches_an_associated_function_through_an_alias() {
    let (rawr, cargo) = both_printed("text.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "1 view pad 20 [Increment] count 0 of 3@50 pad 0 0+1@16 0+2@16 pad 0 [Decrement]"
        ) && rawr.contains(
            "last view pad 20 [Increment] count 2 of 3@50 pad 0 2+1@16 2+2@16 \
             pad 0 Increment@40 Increment@40 Increment@40 Decrement@40 [Decrement]"
        ),
        "{rawr}"
    );
}

/// iced's events and url_handler: the opaque `impl Program` that
/// `application(..)` gives takes a script function as its subscription,
/// which borrows the program's state as `&P::State` and gives a
/// `Subscription<P::Message>`, both read through the opaque program's
/// bounds. The call gives another opaque program, whose `fonts` and `run`
/// are compiled where it was made, and the crate calls the subscription
/// back with the booted state. As url_handler's `text(url)` does, the view
/// gives a widget that borrows a string of the state through an `impl
/// IntoLabel<'a>` its output keeps. The output is `cargo run`'s.
#[test]
fn an_opaque_program_takes_a_subscription_of_its_state() {
    let (rawr, cargo) = both_printed("subscription.rs", "Listen");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("heard Ticked(3)")
            && rawr.contains("1 view pad 0 <Listen:0> 'tick 3 of 6'@48")
            && rawr.contains("1 message Toggled(true)")
            && rawr.contains("last view pad 0 <x Listen:0> 'tick 3 of 6'@48"),
        "{rawr}"
    );
}

/// iced's pane_grid (#1234): a closure the crate calls on another thread
/// gives `Some` of a program message holding crate values, which move with
/// it to the program's thread, as `Some` or `None` is read at once. The
/// output is `cargo run`'s.
#[test]
fn a_message_a_closure_gives_on_another_thread_keeps_its_crate_values() {
    let (rawr, cargo) = both_printed("hotkeys.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("heard SplitFocused(Vertical)\nsplit Vertical true")
            && rawr.contains(
                "heard Both([Horizontal, Vertical])\nsplits [Vertical, Horizontal, Vertical]"
            ),
        "{rawr}"
    );
}

/// iced's todos (#1050): `Application::presets` takes `impl
/// IntoIterator<Item = Preset<P::State, P::Message>>`, whose item names the
/// opaque program's own state and message, read through the opaque
/// program's bounds as rustc reads them. A program function gives the
/// presets, and the crate boots each one and shows its view. The output is
/// `cargo run`'s.
#[test]
fn an_opaque_program_takes_presets_of_its_own_state() {
    let (rawr, cargo) = both_printed("presets.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("preset Empty view pad 0 'Nothing to do'@16")
            && rawr.contains("preset Carl Sagan view pad 0 'Create the universe'@16"),
        "{rawr}"
    );
}

/// iced's game_of_life, arc and delineate: `Application::theme` takes `impl
/// ThemeFn<State, Theme>`, which the crate implements for its `Theme` and
/// for every `Fn(&State)` whose output turns into an `Option<Theme>`. The
/// theme itself fills it by the first impl, and a script function of the
/// state by the second. Both outputs are `cargo run`'s.
#[test]
fn a_parameter_of_a_trait_over_fn_takes_any_type_that_implements_it() {
    let (rawr, cargo) = both_printed("theme.rs", "Dark,Dark");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 theme Dark") && rawr.contains("2 theme Dark"),
        "{rawr}"
    );
    let (rawr, cargo) = both_printed("themed.rs", "Dark,Dark");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 theme Light") && rawr.contains("2 theme Dark"),
        "{rawr}"
    );
}

/// iced's `.theme(|state| ..)`, `.subscription(|state| ..)` and
/// `.scale_factor(|_: &State| 2.0)`, and an update closure: closures whose
/// inputs borrow the program's own state, `&State` and `&mut State`, cross
/// as the state's methods do. The update
/// changes the state in place, and the theme and subscription read it.
/// The output is `cargo run`'s.
#[test]
fn closures_that_borrow_the_programs_state_cross_as_its_methods_do() {
    let (rawr, cargo) = both_printed("closures.rs", "Dark,Dark");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("update Ticked(2) after 1")
            && rawr.contains("1 theme Light")
            && rawr.contains("update Toggled(true) after 3")
            && rawr.contains("2 theme Dark")
            && rawr.contains("2 scale 2"),
        "{rawr}"
    );
}

/// iced's `.subscription(|state| ..)` and `.scale_factor(|_| 2.0)` with no
/// annotations. Each closure's input is `&P::State` of the opaque program
/// that `application(..)` returns, and the opaque type's `State = Inferred`
/// binding fills it in, as rustc reads it. The output is `cargo run`'s.
#[test]
fn an_unannotated_closure_takes_its_input_from_the_opaque_programs_state() {
    let (rawr, cargo) = both_printed("inferred.rs", "Dark,Dark");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("update Ticked(2) after 1")
            && rawr.contains("1 theme Light")
            && rawr.contains("2 theme Dark")
            && rawr.contains("2 scale 2"),
        "{rawr}"
    );
}

/// iced's stopwatch: the crate's `time` re-exports std's `Duration` and
/// `Instant`, and the script imports them through it and writes
/// `time::Duration` through the module. `time::every(milliseconds(10))`
/// takes and gives std's types, and a variant's constructor maps each
/// tick's `Instant`, which shows as Rust's `{:?}` shows it. The output is
/// `cargo run`'s, apart from the instants' values, which differ between
/// runs.
#[test]
fn std_types_a_crate_re_exports_cross_by_value() {
    let (rawr, cargo) = both_printed("time.rs", "Stop");
    assert_eq!(instants(&rawr), instants(&cargo));
    let tick = "heard Tick(Instant { .. })";
    assert_eq!(
        instants(&rawr),
        format!(
            "title crossing::Stopwatch\nfonts 0\n{tick}\n{tick}\n{tick}\n\
             1 view pad 0 <x Stop:0> '20 ms in 3 ticks'@16\n1 message Toggle(false)\n\
             last view pad 0 <Stop:0> '20 ms in 3 ticks'@16"
        )
    );
}

/// `text` with each `Instant { tv_sec: 7, tv_nsec: 8 }` shown without its
/// values, which differ between runs.
fn instants(text: &str) -> String {
    text.lines()
        .map(|line| match (line.find("Instant { "), line.rfind(" })")) {
            (Some(start), Some(end)) if start < end => {
                format!("{}Instant {{ .. }}{}", &line[..start], &line[end + 2..])
            }
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// iced's solar_system (#979): `timed(..)` gives an opaque program whose
/// message is the tuple `(Message, Instant)`, and `.theme(..)` gives
/// another that reads it from the first as `P::Message`. The output is
/// `cargo run`'s, apart from the instants' values.
#[test]
fn an_opaque_program_whose_message_is_a_tuple_takes_a_theme() {
    let (rawr, cargo) = both_printed("timed.rs", "Dark,Dark");
    assert_eq!(instants(&rawr), instants(&cargo));
    assert!(
        instants(&rawr).contains(
            "1 theme Light\n1 message (Toggled(true), Instant { .. })\n2 view pad 0 <x Dark:0>\n2 theme Dark"
        ),
        "{rawr}"
    );
}

/// iced's clock (#981): native code calls the subscription's closure,
/// `|_| Message::Tick(Stamp::now())`, and the boot task's function on
/// another thread, and each reads the crate's clock into the message. The
/// message is made where it is delivered, where the owner's arena holds
/// the crate value. The output is `cargo run`'s.
#[test]
fn a_closure_native_code_calls_on_another_thread_makes_a_crate_value() {
    let (rawr, cargo) = both_printed("clock.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("boot message Tick(Stamp(2))\nheard Tick(Stamp(3))")
            && rawr.ends_with("last view pad 0 '4 ticks at 5'@16"),
        "{rawr}"
    );
}

/// iced's exit and events: `latest().and_then(..)` takes a script closure,
/// or the generic crate function `close` as `and_then(window::close)` does,
/// on a `Task<Option<u32>>`. The crate overloads `and_then` by the task's
/// type argument across `impl<T> Task<Option<T>>` and `impl<T, E>
/// Task<Result<T, E>>`, and the receiver's type picks the first, so
/// `close` is `close::<Message>`. The crate delivers the message a task
/// gives. The output is `cargo run`'s.
#[test]
fn a_method_of_an_impl_over_an_option_of_its_parameter_runs() {
    let (rawr, cargo) = both_printed("exit.rs", "Exit,Exit");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 message Confirm(true)")
            && rawr.contains("task message Closed(7)")
            && rawr.contains("2 message Confirm(false)\nclose 7")
            && rawr.contains("last view pad 0 <Exit:0> \"7\""),
        "{rawr}"
    );
}

/// iced's exit, toast, game_of_life and modal: an array literal passed to
/// `padding(impl Into<Padding>)` is the `[u16; 2]` or `[f32; 2]` of the
/// `From` impl its elements fit, a range of floats fills a slider's
/// `RangeInclusive<T>` at the `f64` or `f32` its value gives `T`, and
/// `Some(color.into())` where a field takes `Option<Background>` converts
/// through `Background`'s `From<Color>`, which the program never names.
/// iced's todos: `Id::from` has a `From<&'static str>` and a `From<String>`
/// impl, both Rawr's string, and the argument's type picks one (T580).
/// The output is `cargo run`'s.
#[test]
fn arrays_float_ranges_and_into_cross_as_rustc_converts_them() {
    let (rawr, cargo) = both_printed("conversions.rs", "pair,slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("[pair 10x20 None/0] [one 4x4 None/0] [floats 1.5x2.5 None/0]")
            && rawr.contains("[static-literal 0x0 None/0] [owned-task-1 0x0 None/0]")
            && rawr.contains("2 message Timeout(30.0)")
            && rawr.contains("1.0..=30.0@30.0/None 0.0..=1.0@0.5/None")
            && rawr.contains("[dim 0x0 Some(Color(Color { r: 9, g: 0, b: 0 }))/0]"),
        "{rawr}"
    );
}

/// iced's clock, game_of_life and the_matrix: a number's `.into()` where a
/// crate struct's field takes a `Pixels` goes through the `From` impl that
/// takes it, and a literal's type is the one that impl fixes: `14.0` an
/// `f32`, `20` a `u32` (#426). The output is `cargo run`'s.
#[test]
fn a_numbers_into_reaches_a_crate_fields_from_as_rustc_infers_it() {
    let (rawr, cargo) = both_printed("pixels.rs", "grow");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("content: \"computed\", size: Pixels(11.0)")
            && rawr.contains("content: \"literal\", size: Pixels(14.0)")
            && rawr.contains("content: \"integer\", size: Pixels(20.5)")
            && rawr.contains("content: \"constant\", size: Pixels(10.0)"),
        "{rawr}"
    );
}

/// iced's layout: `fn square(size: impl Into<Length> + Copy) -> impl
/// Widget<Message>` passes its parameter of two bounds to a crate's
/// methods that take `impl Into<Length>`, and is compiled per instantiation
/// (D629) though its output is opaque. An `f32`, a `u32` and the crate's
/// own type fill it through the crate's `From` impls, and the literal `50`
/// is the `u32` of the one impl of its kind, as rustc selects it. A crate
/// function taking `impl Into<Pixels> + Copy` takes the same (#912). The
/// output is `cargo run`'s.
#[test]
fn a_parameter_of_two_bounds_takes_what_a_crates_from_impls_convert() {
    let (rawr, cargo) = both_printed("squared.rs", "grow");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "last view pad 0 [grow 0x0 None/0] [program 15x15] [pixels 3x3] \
             [integer 7.5x7.5] [literal 50.5x50.5] [crate 30x30] \
             [crate-integer 4.5x4.5] [crate-literal 4.5x4.5]"
        ),
        "{rawr}"
    );
}

/// iced's table: `table(columns, &self.events)` takes a borrowed `Vec` as
/// rows of `impl IntoIterator<Item = T>`, whose items are the `&Event` the
/// columns' `|event: &Event|` closures take, so `T: Clone` holds of a
/// script type that isn't `Clone`. The script's `Event` is also a name the
/// runner's own code used. The output is `cargo run`'s.
#[test]
fn a_borrowed_vec_fills_rows_a_crate_takes_by_into_iterator() {
    let (rawr, cargo) = both_printed("table.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("last view pad 0 \"Events\" |Name|Time|Talk|45 min|Workshop|120 min|"),
        "{rawr}"
    );
}

/// iced's table again, with the table alone in the view's `column!`: a
/// widget with no lifetime, made from what the view borrows, is the view's
/// value without anything else that borrows (#352). The output is `cargo
/// run`'s.
#[test]
fn a_view_of_only_a_lifetime_free_widget_runs() {
    let (rawr, cargo) = both_printed("table_alone.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("last view pad 0 |Name|Time|Talk|45 min|Workshop|120 min|"),
        "{rawr}"
    );
}

/// iced's table and arc: rows of script values whose fields hold std's
/// `Duration` and `Instant` cross to the crate, which checks each as Rust's
/// own type. The output is `cargo run`'s.
#[test]
fn script_rows_holding_std_time_values_cross() {
    let (rawr, cargo) = both_printed("table_timed.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("|Name|Time|Talk|45 min true|Workshop|120 min true|"),
        "{rawr}"
    );
}

/// iced's bezier_tool: the view is a container, invariant in its lifetime,
/// of a hover whose base has no lifetime and whose top is `None` at first,
/// so nothing the view passes the hover borrows. It is made at the view's
/// lifetime all the same (#352). The output is `cargo run`'s.
#[test]
fn a_view_whose_widgets_borrow_nothing_is_made_at_the_views_lifetime() {
    let (rawr, cargo) = both_printed("hovered.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view container 20 hover rule 0 none")
            && rawr.contains("4 message Decrement"),
        "{rawr}"
    );
}

/// iced's sierpinski_triangle: the crate's `Point` shares its name with a
/// type of another crate the root reaches only through a signature, as
/// iced's does tiny_skia's. Its fields, its pattern and its literal are the
/// crate's own type's (#954). The output is `cargo run`'s.
#[test]
fn a_crate_type_whose_name_another_crates_type_shares_crosses() {
    let (rawr, cargo) = both_printed("pointed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "1.5 2 1.5 2 1.5 2", "{rawr}");
}

/// iced's gradient and custom_quad: a crate macro makes a crate struct
/// through the crate's own path (`$crate::Color`), and the program reads
/// its fields, destructures it and builds one from it at its public path:
/// one type at two paths (#953). The output is `cargo run`'s.
#[test]
fn a_crate_value_a_macro_made_crosses_at_its_public_path() {
    let (rawr, cargo) = both_printed("colored.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "red 1 of 1 2 3\nlighter rgb(9, 2, 3)", "{rawr}");
}

/// iced's solar_system: a borrow of a crate value the program keeps fills
/// a crate function's `impl Into<Handle>` and `impl Into<Image>`, through
/// the crate's `From<&Handle>` impls, as `canvas::Image::new(&self.earth)`
/// does (#954). The output is `cargo run`'s.
#[test]
fn a_borrowed_crate_value_fills_an_impl_into_parameter() {
    let (rawr, cargo) = both_printed("imaged.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr, "sun turned 0\nearth turned 2.5\nsun turned 0\ntrue",
        "{rawr}"
    );
}

/// iced's canvas aliases: `canvas::Cache` passes its parameter on to a
/// type of another crate that no path of the root reaches, and defaults it
/// to the root's `Renderer` alias. A derived `Default` builds a field of it,
/// and its associated functions and methods run. `canvas::Geometry` names
/// an associated type of that renderer, at its default a type of its own,
/// which `Cache::sketch` gives as `R::Geometry` (T318), and `Note<'a>`
/// passes a lifetime and a defaulted theme on, as iced's `widget::Text`
/// does. `Option::map` takes a crate function and a crate method named as
/// values (T327). `Cache::new()` with no annotation is the `Cache` a later
/// use fixes, as the_matrix's `resize_with` fills a `Vec<canvas::Cache>`
/// (T413). `Path::new(|p| ..)`, re-exported from that crate, takes a
/// one-shot closure the crate lends a `&mut Builder`, as bezier_tool's does
/// (#477). `Frame::with_save(&mut self, f: impl FnOnce(&mut Frame) -> R)`
/// lends its own receiver to the closure and gives back its output, as
/// iced's clock calls it (#586). `Geometry<Renderer>`, written with the
/// alias's defaulted argument as sandpiles writes it, is the same type
/// (#317). The output is `cargo run`'s.
#[test]
fn aliases_that_default_to_an_alias_or_name_an_associated_type_cross() {
    let (rawr, cargo) = both_printed("canvas.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "sketch circle,line\ncleared []\nboard frame\nprimary arc\nprimary hand\n\
         primary dial\nprimary face\nboard frame,dial\n\
         primary sand\nprimary pile\nprimary grain\ngrouped group\nfresh fresh 3\n\
         Some(7) captured true\nnote 3\npath 1,2\n\
         frame face@0,hour@1,second@2,rim@0 hands 2"
    );
}

/// iced's solar_system (#595): `canvas::LineDash { segments: &[3.0, 6.0],
/// offset: 0 }`, a struct of public fields re-exported from another crate,
/// one of them a slice that borrows for the struct's lifetime, built inside
/// a `Stroke { .., ..Stroke::default() }` and read back. The output is
/// `cargo run`'s.
#[test]
fn a_struct_literal_fills_a_field_that_borrows_a_slice() {
    let (rawr, cargo) = both_printed("dashed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "stroke 2 dash [3.0, 6.0]+1\n[0.5] 0\nstroke 1 dash []+0"
    );
}

/// iced's color_palette (#594): `Hsl::from_color(rgb)` and
/// `Rgb::from_color(hsl)` are palette's blanket `impl<T, U> FromColor<T>
/// for U where U: FromColorUnclamped<T> + Clamp`, applied to each type
/// through each of its `FromColorUnclamped` impls, and the argument picks
/// one, as rustc does. The output is `cargo run`'s.
#[test]
fn an_associated_function_of_a_blanket_impl_runs_on_each_type_it_applies_to() {
    let (rawr, cargo) = both_printed("palette.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "hsl 220 1 0.625\nrgb 0.5000001 0.25 1\nhsl 0 1 0.5");
}

/// iced's color_palette (#628): `Rgb::from_color(lab)` goes through
/// palette's `impl<S> FromColorUnclamped<Lab<<S::Space as
/// RgbSpace>::WhitePoint, f32>> for Rgb<S, f32>`. The projection of a
/// projection is a synthetic parameter the call infers from `S`, as `Srgb`'s
/// `Space` is `Srgb`, whose `WhitePoint` is `D65`, as rustc normalizes it.
/// The output is `cargo run`'s.
#[test]
fn an_impl_whose_input_projects_through_a_projection_converts() {
    let (rawr, cargo) = both_printed("lab.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "rgb 0.6 0.35 0.7\nrgb 0.84999996 0.99999994 0.75");
}

/// iced's color_palette (#617): `Rgb { red, ..Rgb::default() }` reads
/// green and blue from a default whose component type `red` and the return
/// type fix, and `Hsl { lightness, ..hsl }` in a loop copies the other
/// fields out of `hsl` each time. Each field is read as a copy because its
/// type is `Copy` once the body fixes it, as rustc decides it. The output
/// is `cargo run`'s.
#[test]
fn a_struct_update_of_a_generic_crate_struct_copies_its_fields() {
    let (rawr, cargo) = both_printed("updated.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "rgb 0.5 0 0\nstep 1 0.5 0 0\nstep 2 1 0 0\nstep 3 1 0.5 0.5\nhsl 0 1 0.5"
    );
}

/// iced's color_palette (#607): `impl<C: Space + Copy> Picker<C>`'s `fn
/// view(&self, color: C) -> impl Widget<C>` names the impl's parameter in
/// a crate's trait, so it is compiled once per space it is used at, a
/// script type or a crate's. Each instance reads `C::RANGES`, `C::LABEL`
/// and `C::new` as that type's impl of the bound's trait gives them, as
/// rustc resolves them. The output is `cargo run`'s.
#[test]
fn an_associated_item_of_a_generic_parameters_bound_is_its_instances_types() {
    let (rawr, cargo) = both_printed("ranges.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("map pad 0 HSL 0..=360@16 [Increment] 1.0 1.0@16")
            && rawr.contains("4 message Lab(Hsl { hue: 25.0,")
            && rawr.ends_with("[0.75, 0.0, 0.0] [0.0, 0.0, 0.0] [25.0, 0.0, 0.0]@16"),
        "{rawr}"
    );
}

/// iced's geometry (#599): `fn view(..) -> impl Widget<Never>`, iced's
/// `Never` being std's `Infallible`, whose body nests containers around a
/// column. The type the body reveals names `Infallible` as the boxed
/// children's message, and the runner writes it and passes it to the
/// crate's generic items. The output is `cargo run`'s.
#[test]
fn a_view_that_sends_no_message_names_infallible_in_its_revealed_type() {
    let (rawr, cargo) = both_printed("never.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with(
            "last view pad 0 pad 0 pad 0 pad 20 rainbow@16 \
             'In this example we draw a custom widget Rainbow'"
        ),
        "{rawr}"
    );
}

/// iced's geometry (#608, #616): `SolidVertex2D { position, color }` fills
/// a field of an array type and one of a crate type whose own field is
/// private, which only the crate's `pack` makes, and `Mesh::Solid { .. }`
/// a variant's array field. The program is `run((), view)`: the update is
/// the crate's impl for `()`, and the view of `&()` gives widgets that
/// borrow string literals, which the slot's own pins hold.
/// `renderer.with_translation(v, |renderer| renderer.draw_mesh(mesh))`
/// gives the closure the receiver's type for the trait method's `&mut
/// Self`, as rustc does (#624). The output is `cargo run`'s.
#[test]
fn a_vertex_literal_and_a_view_of_unit_state_run() {
    let (rawr, cargo) = both_printed("geometry.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.starts_with(
            "3 vertices, last Some(SolidVertex2D { position: [4.0, 0.0], color: \
             Packed([0.5, 0.25, 0.0, 1.0]) }), 1 triangles at [4.0, 1.5]\n\
             [1.0, 2.0] Packed([0.5, 0.25, 0.0, 1.0])\n\
             drew 3 vertices at [2.0, 3.0]\ntitle ()"
        ) && rawr.ends_with(
            "last view pad 0 pad 20 rainbow@16 'In this example we draw a custom widget \
             Rainbow' 'Move your cursor over it'"
        ),
        "{rawr}"
    );
}

/// A literal of a crate struct whose public field is a function pointer,
/// which rawr can't hold yet, is reported at the literal, naming the field
/// and its type as the crate writes them (#608, D664).
#[test]
fn a_literal_names_the_public_field_rawr_cannot_hold() {
    let stderr = checked("shaded.rs");
    assert!(stderr.contains("main.rs:13:18"), "{stderr}");
    assert!(
        stderr.contains("field `shade` has type `fn(f32) -> f32`, which rawr can't hold yet"),
        "{stderr}"
    );
}

/// fount's `.title("fount – system font")`: iced's `title(impl
/// Title<State>)` is implemented for `&'static str`, so the literal lives
/// for the program, as rustc's literals do. A view calls
/// `Named::with_name(&'static str)` every frame, and a loop calls
/// `heading(impl Heading)`, implemented for `&'static str` alone. Each
/// site holds its literal once, with no copy (#667). The output is `cargo
/// run`'s.
#[test]
fn a_literal_lent_for_the_program_lives_as_long_as_it() {
    let (rawr, cargo) = both_printed("titled.rs", "Count,Count");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.starts_with("headings 42\n")
            && rawr.contains("last view pad 0 <Count:0> \"2\" \"9\"\nnamed fount – system font"),
        "{rawr}"
    );
}

/// A local `String` borrowed where only a `&'static str` goes is rustc's
/// E0597 at the borrow: by a `&'static str` input, and by a type
/// parameter whose trait is implemented for `&'static str` (#667).
#[test]
fn a_borrowed_local_where_a_static_str_goes_does_not_live_long_enough() {
    for (variant, at) in [
        ("lent_name.rs", "main.rs:10:37"),
        ("lent_heading.rs", "main.rs:10:28"),
    ] {
        let stderr = checked(variant);
        let cargo = cargo_s_report(&stderr);
        assert!(
            cargo.contains("error[E0597]: `name` does not live long enough") && cargo.contains(at),
            "{variant}: {stderr}"
        );
    }
}

/// iced's clock: a crate type compared with a number, `rotation < 180.0`,
/// through `PartialOrd<f32>`, whose impl writes only `partial_cmp`, so `<`
/// is std's provided `lt`. The type is also `PartialOrd` with itself, and
/// rustc picks the impl by the right-hand side (T412). The output is
/// `cargo run`'s.
#[test]
fn a_crate_type_compares_with_a_number_through_std_provided_methods() {
    let (rawr, cargo) = both_printed("compared.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "turn left\ntrue false false true false\ntrue false");
}

/// iced's game_of_life: `translation + (cursor - start) * (1.0 / scaling)`
/// on a generic `Point` with two `Sub` impls, `Sub<Point>` whose `Output`
/// is a `Vector` and `Sub<Vector>` whose `Output` is `Self`. Each operator
/// takes the impl its right-hand side picks, and that impl's `Output`, as
/// rustc does (T502). The output is `cargo run`'s.
#[test]
fn an_operator_takes_the_output_of_the_impl_its_operand_picks() {
    let (rawr, cargo) = both_printed("plane.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "Vector { x: 4.5, y: 10.0 }\nPoint { x: 5.5, y: 10.0 } Point { x: 10.0, y: 20.0 }"
    );
}

/// iced's bezier_tool and game_of_life: `.map(Message::Curve)` on a canvas
/// whose program sends its own messages. The crate's `map` gives `Map<Self,
/// F, Message>`, naming the closure's type, and the runner writes the
/// closure as `Box<dyn Fn(Curve) -> Message>` (T417). The runner's own
/// bindings declare `map` as the check's do (T423). `column!` boxes the
/// `Map` beside the controls through `impl<W, A, B, F, Theme, Renderer>
/// Widget<B, Theme, Renderer> for Map<W, F, A>`, whose type names its
/// parameters in another order than the impl declares them, and whose `F`
/// is the boxed closure the receiver holds (T425). The output is `cargo
/// run`'s.
#[test]
fn a_crate_method_whose_output_names_its_closure_type_runs() {
    let (rawr, cargo) = both_printed("mapped.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 message Curve(Drawn(\"Increment\"))")
            && rawr.ends_with("last view pad 0 map canvas[primary curves 4#1] pad 0 4 curves@16"),
        "{rawr}"
    );
}

/// iced's game_of_life (#1060): `grid(..).map(Message::Curve.with(version))`
/// on the `impl Widget<Curve>` a program function returns. The crate's
/// `Function::with` gives an `impl Fn(Curve) -> Message`, which fills
/// `map`'s `F` as itself, as the `Box<dyn Fn(Curve) -> Message>` that
/// `Map<Self, F, Message>` names. The output is `cargo run`'s.
#[test]
fn a_crate_callable_value_fills_a_callable_its_output_names() {
    let (rawr, cargo) = both_printed("mapped_with.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 message Curve(0, Drawn(\"Increment\"))")
            && rawr.ends_with("last view pad 0 map canvas[primary curves 4#1] pad 0 4 curves@16"),
        "{rawr}"
    );
}

/// iced's bezier_tool (#990): `{:?}` of the crate's `Action<Curve>` that
/// publishes a program's curve. The crate's derived `Debug` prints the
/// curve, whose own `Debug` prints the crate's points it holds while the
/// outer value's is printing. The output is `cargo run`'s.
#[test]
fn a_crate_values_debug_prints_the_crate_values_a_program_value_holds() {
    let (rawr, cargo) = both_printed("published.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.starts_with(
            "Action { message: Some(Curve { from: Point { x: 5.0, y: 5.0 }, to: Point { x: \
             45.0, y: 75.0 } }), captured: true }"
        ),
        "{rawr}"
    );
}

/// iced's layout (#991): `container(pin("• (50, 50)").x(..))`, whose
/// argument is a `Pin<&str>`, a crate value whose type has a lifetime only
/// its elided `&str` names. The runner takes it as it takes any value
/// whose type has lifetimes, and builds. The output is `cargo run`'s.
#[test]
fn a_crate_value_whose_type_elides_a_lifetime_fills_a_generic_argument() {
    let (rawr, cargo) = both_printed("pinned.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 container 0 pin 30 '• (50, 50)' [Increment] 30@16"),
        "{rawr}"
    );
}

/// Opaque `impl Widget` outputs as cargo checks them (edition 2024): one
/// made of its parameter (T462) and one of a `&'static str` (T428) take no
/// input's lifetime, and a canvas behind one is mapped (T424) and boxed by
/// `column!` through the trait's own `boxed`, as `Map`'s impl has
/// parameters its type doesn't. A button behind `impl Widget<Part>` is
/// mapped too: its impl of `Widget` is at any renderer, so `map` is called
/// through the trait at the arguments `impl Widget<Part>` fixes (T428). As
/// iced's bezier_tool, the canvas comes from a module's method that borrows
/// its state and the curves, behind `impl Framed<Curve>`, the extension
/// trait `map` belongs to, and its program borrows both (T567). The output
/// is `cargo run`'s.
#[test]
fn opaque_widget_outputs_take_no_lifetime_and_map_as_cargo_checks_them() {
    let (rawr, cargo) = both_printed("opaque.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "1 view pad 0 [Increment] map [Decrement] [Decrement] map canvas[primary curves 0 0#1]"
        ) && rawr.contains("4 message Part(Down)")
            && rawr.ends_with(
                "last view pad 0 [Increment] map [Decrement] [Decrement] map canvas[primary curves 0 0#1]"
            ),
        "{rawr}"
    );
}

/// iced's loupe (#845): `zoomed` gives the program's `Zoomed<impl
/// Widget<Message> + 'a>`, an `impl Trait` inside the output, whose hidden
/// type the checker infers from the body, as rustc does. The output is
/// `cargo run`'s.
#[test]
fn an_opaque_type_inside_a_program_functions_output_is_the_bodys_type() {
    let (rawr, cargo) = both_printed("wrapped.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 pad 0 [Increment] 2@50 pad 3 [Decrement]"),
        "{rawr}"
    );
}

/// iced's ferris and layout (#885): `explain`, a method of the
/// blanket-implemented `Framed`, gives an `impl Widget` from the trait's own
/// default body. It sits in `column!` as it is, is boxed through `Widget`'s
/// own `boxed`, and through `Framed::framed`, which the blanket impl over
/// `Widget` gives it. The output is `cargo run`'s.
#[test]
fn a_trait_methods_opaque_output_from_its_default_body_is_usable() {
    let (rawr, cargo) = both_printed("explained.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with(
            "last view pad 0 [Increment] 2@50 explain #ffffff rule 1 explain #0000ff rule 2 \
             explain #00ff00 rule 3 [Decrement]"
        ),
        "{rawr}"
    );
}

/// iced's layout (#936): `self.example().explain(color).boxed()` calls
/// `explain` on an `Element<'_, ..>` a method of the program gives, so the
/// opaque type it gives borrows what the element borrows. The runner holds
/// it in the frame's region by the family a wrapper function's output
/// projects, as Rust code can't name the type. The output is `cargo run`'s.
#[test]
fn an_opaque_output_that_borrows_an_element_is_held_in_its_region() {
    let (rawr, cargo) = both_printed("explained_element.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 [Increment] 2@50 explain #0000ff rule 4 [Decrement]"),
        "{rawr}"
    );
}

/// iced beside iced_test (#1660): `tester` re-exports `parts`, as
/// iced_test re-exports iced_core, and no path of it reaches `Framed`, as
/// none of iced_test's reaches iced_widget. `explain` on `parts`'s `Map`
/// gives one opaque type, declared beside `Framed`'s method in `events` and
/// beside `Map` in `tester`, each naming its parameters after its own
/// declaration. The output is `cargo run`'s.
#[test]
fn an_opaque_output_two_crates_reach_is_one_type() {
    let manifest = CROSSING.read("Cargo.toml").replace(
        "[dependencies]",
        "[dependencies]\ntester = { path = \"tester\" }",
    );
    let main = CROSSING.read("variants/tested.rs");
    let root = CROSSING.lease(&[("Cargo.toml", &manifest), ("src/main.rs", &main)]);
    let (rawr, cargo) = both_printed_in(&root, "", &[]);
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 [Increment] 3@50 explain #0000ff map rule 4"),
        "{rawr}"
    );
}

/// iced_test's `ui.click("Increment")` (#1678): `click` is generic over a
/// `Selector` the crate implements for `&str` and for closures over a
/// `Candidate<'_>`, a form rawr doesn't take, and a string literal fills
/// it as itself. The output is `cargo run`'s.
#[test]
fn a_selector_a_closure_also_fills_takes_a_string() {
    let (rawr, cargo) = both_printed("clicked.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("click Err(NotFound(\"text \\\"Reset\\\"\"))\nmessage Increment")
            && rawr.ends_with("message Decrement\nvalue 1"),
        "{rawr}"
    );
}

/// iced_test's `ui.click(|candidate| ..)` (#1694): `click`'s `Selector` is
/// implemented for `FnMut(Candidate<'_>) -> Option<T>`, and a closure over
/// the lifetime-carrying candidate fills it, one capturing what it looks
/// for. The output is `cargo run`'s.
#[test]
fn a_closure_over_a_lifetime_carrying_candidate_fills_a_selector() {
    let (rawr, cargo) = both_printed("clicked_closure.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.starts_with("click Ok(\"Increment\")\nclick Ok(\"Decrement\")\n")
            && rawr.contains("click Err(NotFound(\"a closure\"))\nmessage Increment")
            && rawr.ends_with("message Decrement\nvalue 0"),
        "{rawr}"
    );
}

/// iced_test's `ui.simulate(click())` (#1679): the opaque `impl
/// Iterator<Item = Event>` `click()` gives fills `simulate`'s `impl
/// IntoIterator<Item = Event>`, from a call and from a local. The output
/// is `cargo run`'s.
#[test]
fn an_opaque_iterator_fills_an_iterable_parameter() {
    let (rawr, cargo) = both_printed("simulated.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("message Increment\nmessage Decrement\nmessage Decrement\nvalue -1"),
        "{rawr}"
    );
}

/// iced_test's `simulator(counter.view())` beside iced (#1680): `tester`
/// takes any `parts` widget, and no impl its facade can name fixes the
/// theme and renderer of the widget `events` makes. The view's `impl
/// Shown<Message>`, a method's and a function's, fixes them, the trait's
/// defaults, with no annotation. The output is `cargo run`'s.
#[test]
fn a_views_opaque_bound_fixes_what_its_value_fills() {
    let manifest = CROSSING.read("Cargo.toml").replace(
        "[dependencies]",
        "[dependencies]\ntester = { path = \"tester\" }",
    );
    let main = CROSSING.read("variants/sighted.rs");
    let root = CROSSING.lease(&[("Cargo.toml", &manifest), ("src/main.rs", &main)]);
    let (rawr, cargo) = both_printed_in(&root, "", &[]);
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("sight badge 3\nsight badge Counter"),
        "{rawr}"
    );
}

/// `outlined` gives an opaque type of a type no public path names, on an
/// `Element<'_, ..>`, held in its region as `explain`'s is (#936). It is a
/// default method of the trait `explain` is one of, and each method's
/// opaque type has members of its own (#943). The output is `cargo run`'s.
#[test]
fn an_opaque_output_of_a_private_type_that_borrows_an_element_is_held() {
    let (rawr, cargo) = both_printed("outlined_element.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 [Increment] 2@50 outlined rule 4 [Decrement]"),
        "{rawr}"
    );
}

/// iced's markdown (#1098): `titled(title, on_press)` takes two `impl
/// Trait` inputs, one a callable, as `code_block` does, and `explain` on
/// what it gives names that opaque type as its `Self` with the arguments
/// the function's output has. The output is `cargo run`'s.
#[test]
fn a_member_of_an_opaque_type_that_leaves_out_a_callable_names_it_whole() {
    let (rawr, cargo) = both_printed("titled_explained.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 [Increment] 3@50 explain #00ff00 titled code"),
        "{rawr}"
    );
}

/// `explain` on what `explain` gives on an `Element<'_, ..>` (#936): the
/// outer opaque type takes the inner one, and the region holds it boxed
/// through a trait of the runner's, as no wrapper over another compiles.
/// The output is `cargo run`'s.
#[test]
fn an_opaque_output_of_an_opaque_output_that_borrows_is_held_boxed() {
    let (rawr, cargo) = both_printed("explained_twice.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with(
            "last view pad 0 [Increment] 2@50 explain #00ff00 explain #0000ff rule 4 [Decrement]"
        ),
        "{rawr}"
    );
}

/// Two `let`s of an invariant text that borrows a string, passed to one
/// call, and two of what `explain` gives on it, passed to another (#1335):
/// Rust infers one lifetime for both borrows. In a view both are made in
/// the view's region; out of one, the first `let` opens a scope frame and
/// the second joins its region, a literal's too, so the call takes both at
/// one lifetime. The element the frame holds is lent to a crate function
/// by `&` and to its own `labels(&mut Vec<String>)` (#1366, #1369), and two
/// values of one opaque type go to one call, `hover(below, above)` (#1368).
/// The output is `cargo run`'s.
#[test]
fn two_lets_of_an_invariant_or_opaque_type_go_to_one_call() {
    let (rawr, cargo) = both_printed("joined.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.starts_with(
            "main pad 0 'left' 'right' [\"pad 0\", \"'left'\", \"'right'\"]\n\
             main pad 0 'left' 'right'\n\
             main hover explain #0000ff rule 1 explain #00ff00 rule 2\n\
             main pad 0 explain #0000ff 'left' explain #00ff00 'right'\n"
        ),
        "{rawr}"
    );
    assert!(
        rawr.ends_with(
            "last view pad 0 [Increment] pad 0 'count' 'presses' 2@50 pad 0 explain #0000ff 'first' explain #00ff00 rule 2 [Decrement]"
        ),
        "{rawr}"
    );
}

/// What `explain` gives on an element that borrows a local, kept past the
/// block the local lives in, is rustc's E0597 at the borrow (#936).
#[test]
fn an_opaque_output_that_outlives_what_it_borrows_does_not_live_long_enough() {
    let stderr = checked("explained_outlived.rs");
    assert!(
        stderr.contains("error[E0597]: `local` does not live long enough"),
        "{stderr}"
    );
}

/// iced's loupe (#885, #913): the program's own `Zoomed<W>` implements the
/// crate's `Widget` for any `W: Widget` through `W`'s methods, and holds
/// what `explain` gives of a rule. The runner compiles the impl for each
/// type it is used at, and names the opaque type in `Zoomed<impl Widget>`
/// by a name of its own, as the proxy holds only the script's value. Its
/// `self.content.labels(labels)` reaches the `&mut Vec<String>` the crate
/// lent it where it is. The output is `cargo run`'s.
#[test]
fn a_program_impl_at_a_crates_opaque_type_runs() {
    let (rawr, cargo) = both_printed("zoomed_rule.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.contains("zoom 3 explain #000000 rule 4"), "{rawr}");
}

/// As `zoomed_rule.rs`, with `zoomed` generic over its message (#1218):
/// a call takes the message from its `impl Widget<Message>` argument's
/// impl, or from a turbofish. The output is `cargo run`'s.
#[test]
fn a_generic_functions_parameter_is_inferred_through_an_impl_trait_arguments_bound() {
    let (rawr, cargo) = both_printed("zoomed_generic.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("zoom 3 explain #000000 rule 4 zoom 2 explain #000000 rule 5"),
        "{rawr}"
    );
}

/// iced's loupe itself (#913): as `zoomed_rule.rs`, but the button that
/// `explain` takes borrows the view, so what it gives is a value of the
/// view's region, which the program's `Zoomed` holds and the crate reaches
/// after the view returns: the slot keeps the region as long as the view's
/// state is unchanged (#1217). The output is `cargo run`'s.
#[test]
fn a_program_impl_at_a_crates_opaque_type_of_the_views_values_runs() {
    let (rawr, cargo) = both_printed("zoomed.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("zoom 3 explain #000000 [Increment]"),
        "{rawr}"
    );
}

/// iced's download_progress (#568): the program's own native
/// `download(url: impl AsRef<str>) -> impl Steps` gives an opaque type
/// that captures its `impl AsRef<str>` input, which Rawr fills with a
/// `String`, so the opaque type takes no argument for it. The value fills a
/// crate function's generic `S`, and a variant takes the last step. The
/// output is `cargo run`'s.
#[test]
fn a_native_opaque_output_of_a_string_input_fills_a_crate_generic() {
    let (rawr, cargo) = both_printed("stepped.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Finished(27))\nSome(Finished(0))");
}

/// iced's `Task::sip` shape: the program's own native `impl Sip<Item =
/// u32>` fills `S`, and a variant fills `impl FnMut(S::Item) -> T`, whose
/// input the opaque type's binding decides. The output is `cargo run`'s.
#[test]
fn a_closure_takes_the_item_a_native_opaque_type_binds() {
    let (rawr, cargo) = both_printed("sipped.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Downloading(18))");
}

/// iced's `Task::sip` also takes `impl FnOnce(<S as Future>::Output) ->
/// T` with `S: Core`: a std trait's associated type of its own type
/// parameter. The program's own native `impl Straw<u32, u32, String>` fills
/// `S`, and binds `Future::Output` through `Sipper` and `Core`, as sipper's
/// traits do. Two variants fill the two callables, and the second takes the
/// `Result<u32, String>` that binding decides, as rustc reads it (#621). The
/// output is `cargo run`'s.
#[test]
fn a_closure_takes_the_output_a_native_opaque_future_binds() {
    let (rawr, cargo) = both_printed("finished.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Finished(Ok(27)))");
}

/// iced's gallery (#637): `Task::future(image.download(size))`, where the
/// program's own native method gives `impl Straw<Bytes, u32, String>`, so
/// `Task::future`'s T is the `Result<Bytes, String>` the opaque type's
/// `Future` binds through `Straw`, `Sipper` and `Core`, as rustc reads it.
/// `.and_then` and `.then` take it from there. The output is `cargo run`'s.
#[test]
fn a_native_opaque_future_fills_a_future_parameter() {
    let (rawr, cargo) = both_printed("futured.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "Some(Downloaded(Ok(Bytes(40))))\nSome(Downloaded(Err(\"empty\")))"
    );
}

/// iced's color_palette (#638): palette's `ShiftHue::shift_hue(self, amount:
/// Self::Scalar)` and `Lighten::lighten`, whose input is the trait's own
/// associated type that each impl sets (`type Scalar = T`). The receiver's
/// type decides the amount's, as rustc reads it. The output is `cargo
/// run`'s.
#[test]
fn a_method_takes_its_traits_associated_type_of_the_receiver() {
    let (rawr, cargo) = both_printed("hued.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.starts_with("hue 220 85"), "{rawr}");
}

/// iced's color_palette (#727): `hsl.shift_hue(-105.0).darken(0.075)`,
/// where palette's blanket `impl<T: Lighten> Darken for T { type Scalar =
/// T::Scalar; }` applies to `Hsl` through its own `Lighten` impl. The
/// application keeps `Scalar`, resolved in that impl's context, so the
/// factor is `<Hsl as Lighten>::Scalar`, as rustc normalizes it. The output
/// is `cargo run`'s.
#[test]
fn a_blanket_impls_associated_type_resolves_through_the_impl_it_applies_by() {
    let (rawr, cargo) = both_printed("darkened.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "hsl 115 1 0.596875");
}

/// iced's gallery (#728): `Task::sip(image.download(..),
/// Message::BlurhashDecoded.with(id), Message::ThumbnailDownloaded.with(id))`
/// passes two closures that capture `id`. An item of several callable
/// parameters takes a capturing closure in each, by that parameter's own
/// call, and beside a script function that fills one as a slot. The
/// closure's `Result<Bytes, String>` input holds a native value, which
/// crosses through the arena. The output is `cargo run`'s.
#[test]
fn capturing_closures_fill_each_of_several_callable_parameters() {
    let (rawr, cargo) = both_printed("withed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "Some(ThumbnailDownloaded(4, Ok(Bytes(21))))\nSome(Failed(Err(\"empty\")))\n\
         Some(ThumbnailAllocated(4, 9))"
    );
}

/// iced's gallery (#597): `Message::BlurhashDecoded.with(id)` through a
/// crate trait shaped like iced's `Function`, whose blanket impl over
/// `Fn(A, B) -> O` gives every callable `with`, on a variant's constructor,
/// a plain function and a capturing closure. Each runs the crate's own
/// `Function::with`, and the `impl Fn(B) -> O` it returns fills
/// `Task::finish`'s and `Task::map`'s callable parameters as itself. No
/// trait of a crate path of its own is special. The output is `cargo run`'s.
#[test]
fn a_trait_method_over_fn_runs_on_functions_and_its_callable_crosses() {
    let (rawr, cargo) = both_printed("functioned.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "Some(ThumbnailDownloaded(4, Ok(Bytes(21))))\nSome(Tagged(50, 9))\nSome(Tagged(104, 2))"
    );
}

/// iced's game_of_life (#1043, #1051): `rows.cartesian_product(columns)` of
/// two let-bound `a..=b` ranges through a crate trait shaped like
/// itertools' `Itertools`, which a blanket impl gives every `Iterator` and
/// a `use` in the function's body brings into scope. A program's iterator
/// crosses as a native iterator that pulls from it on each `next()`: a
/// mapped iterator's closure runs as the crate pulls, an unbounded one is
/// pulled only as far as `take` asks, and the `Product` the crate gives
/// back is an iterator the program drives, with a `for` loop, `map` and
/// `filter`. That crate iterator fills the trait's `Self` and a crate
/// function's `I: Iterator` as itself. The output, and so the order of
/// each side effect, is `cargo run`'s.
#[test]
fn a_program_iterator_crosses_lazily_into_a_crate_traits_blanket_method() {
    let (rawr, cargo) = both_printed("paired.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "8 Some(Cell { i: -1, j: 4 })\nmade\ncolumn a\nrow 0\nSome((0, 'a'))\ncolumn b\n0b\n\
         column a\nrow 1\n10a\ncolumn b\n10b\ncolumn a\npull 1\npull 2\n\
         [(1, 0), (1, 1), (2, 0)]\n05x 05y 06x 06y 15x 15y 16x 16y \n6 4\n5"
    );
}

/// A program's `a..=b` of an integer other than `i64` (#1055) crosses as a
/// `RangeInclusive` of its own type where a crate's `J: IntoIterator` or `T`
/// takes it by value, so rustc fills the parameter with it, and as an
/// iterator of that type where a crate takes `I: Iterator`. A range a type
/// parameter gives back is the program's range of that type. An `a..b` of
/// one, an iterator, fills either. The output is `cargo run`'s.
#[test]
fn a_range_of_a_narrower_integer_crosses_as_a_range_of_its_type() {
    let (rawr, cargo) = both_printed("kinded.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "[(0, -1), (0, 0), (0, 1), (1, -1), (1, 0), (1, 1)]\n3 4\n[10, 11, 12] [-2, -1, 0]\n\
         9 2..=4 4"
    );
}

/// A crate function over `I: Iterator` (#1051) takes an iterator: an
/// array there is rustc's E0277, as `cargo build` reports it, where a `J:
/// IntoIterator` takes one.
#[test]
fn an_array_where_a_crate_takes_an_iterator_is_e0277() {
    let stderr = rejected_as_cargo("paired_array.rs", "E0277");
    assert!(stderr.contains("Iterator"), "{stderr}");
}

/// fount's loading program: iced's `Task::future(impl Future<Output = T>)`
/// of an async block that awaits a crate's `async fn`, then a closure that
/// takes the block's output and matches on it. The output is a `Result`
/// holding the crate's own error, so it crosses as Rust's own type, made on
/// the thread that holds the program's values (#652). The block also calls
/// `std::fs::read` by its full path (#659). The output is `cargo run`'s.
#[test]
fn a_closure_takes_the_output_of_an_async_block_a_future_parameter_binds() {
    let (rawr, cargo) = both_printed("loaded.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "Some(Loaded(7))\nSome(Failed(\"no font named \\\"\\\"\"))\nSome(Read(true))"
    );
}

/// iced's toast (#1073): `Task::perform(future, f)` takes a future and a
/// closure over its output, as `Task::perform(tokio::time::sleep(..), move
/// |_| Message::Close(id))` does. A crate's own future fills the future
/// parameter, and so does an async block, each beside a capturing closure,
/// whose input is the future's output. The output is `cargo run`'s.
#[test]
fn a_closure_beside_a_future_parameter_takes_the_futures_output() {
    let (rawr, cargo) = both_printed("performed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Close(7))\nSome(Waited(8, 10))");
}

/// A crate function whose output future's `Output` names its future
/// parameter's `S::Output`, as futures' adapters keep their input's output:
/// that projection is the future's own synthetic `SOutput`, not a second
/// one of the same name (E0403 in inboard's Bind). The output is `cargo
/// run`'s.
#[test]
fn a_future_parameters_output_named_in_the_output_future_is_its_own() {
    let (rawr, cargo) = both_printed("passed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Waited(4, 15))");
}

/// A crate's own generic may be named as Rawr's synthetic parameters were:
/// `SOutput` beside a future parameter `S`, and `impl_1` beside an
/// argument's `impl Trait` at index 1. Each synthetic is named by what it
/// is, which no Rust name spells, so neither clashes (E0403). The output is
/// `cargo run`'s.
#[test]
fn a_crates_generic_named_as_a_synthetic_parameter_is_its_own() {
    let (rawr, cargo) = both_printed("passed_named.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Waited(15, 7))\n7");
}

/// A crate future's output that holds a `&'static str` (#1700):
/// `passed_beside(.., "beside")` gives `(u32, &str)`, and a script closure
/// takes it apart and keeps the string, as rustc allows. The output is
/// `cargo run`'s.
#[test]
fn a_futures_output_holding_a_static_str_reaches_a_closure() {
    let (rawr, cargo) = both_printed("passed_label.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "Some(Waited(15, \"beside\"))");
}

/// iced's game_of_life (#1659): `Task::perform(task, Message::Grid.with(version))`
/// takes the `impl Future + use<>` a program method gives from an async
/// block beside a crate callable value, and a capturing closure in its
/// place. Check and run key the call alike. The output is `cargo run`'s.
#[test]
fn a_programs_opaque_future_beside_a_crate_callable_value_performs() {
    let (rawr, cargo) = both_printed("ticked.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "Some(Grid(1, Ticked { cells: 15 }))\nSome(Ticked(2, 10))\nqueued 2"
    );
}

/// iced's gallery (#570): `card(image, preview, ..).boxed()` in a `map`
/// closure over the program's own native images, where `card` is written
/// after the view and borrows the image and an `Option<&String>` an `if`
/// chooses, behind `impl Widget<Message> + 'a`. The receiver of `boxed` is
/// `card`'s opaque type, as rustc infers it. The output is `cargo run`'s.
#[test]
fn a_borrowing_opaque_widget_is_boxed_in_a_closure_before_its_function() {
    let (rawr, cargo) = both_printed("gallery.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 [Increment] pad 1 ...@16 ...@16")
            && rawr.ends_with(
                "last view pad 0 [Increment] pad 1 pad 2 pad 0 [image#0] preview0@16 \
                 pad 2 [image#1] pad 2 pad 0 [image#2] preview2@16"
            ),
        "{rawr}"
    );
}

/// iced's gallery (#623): `Viewer::view` gives `Option<impl
/// Widget<Message>>`, and its body returns `Some(opaque(..))`, whose theme
/// and renderer the crate's `opaque` leaves open. The written `impl
/// Widget<Message>` fixes them by the trait's defaults, as it does for a
/// bare `impl Widget<Message>`, and as rustc does. The output is `cargo
/// run`'s.
#[test]
fn an_option_of_an_opaque_widget_takes_the_written_bounds_defaults() {
    let (rawr, cargo) = both_printed("viewer.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("2 view pad 0 [Increment] none")
            && rawr.contains("3 view pad 0 [Increment] opaque pad 0 0..=10@2/None")
            && rawr.ends_with("last view pad 0 [Increment] opaque pad 0 0..=10@3/None"),
        "{rawr}"
    );
}

/// iced's gallery (#626): `stack![content, viewer]` takes the `Option<impl
/// Widget<Message>>` itself, through the crate's `impl Widget for
/// Option<T>`. It is `None` before anything made a value of the opaque
/// type, and the call that boxes it runs at that type all the same, from
/// the table the call that makes such values compiles. The output is
/// `cargo run`'s.
#[test]
fn an_option_of_an_opaque_widget_crosses_whole_while_it_is_none() {
    let (rawr, cargo) = both_printed("stacked.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 [Increment] none")
            && rawr.ends_with("last view pad 0 [Increment] opaque pad 0 0..=10@3/None"),
        "{rawr}"
    );
}

/// iced's modal (#955), without what borrows the view: a script function
/// `wrapped<'a, Message>(content: impl Widget<Message> + 'a, ..) -> impl
/// Widget<Message> + 'a` wraps an opaque widget in a container and a mouse
/// area, whose types declare a lifetime, and puts that in another opaque
/// widget. The runner names each type with its lifetimes, as rustc does: a
/// witness of `MouseArea<'_, Message, Container<'_, _>>` names them, and so
/// does each receiver that holds the opaque type. Beside it, a field
/// borrows the state's email for the view's lifetime through `Field<'a,
/// ..>::value(self, &'a str) -> Self`, a crate method rustc reads as keeping
/// it borrowed (#598). The output is `cargo run`'s.
#[test]
fn a_script_function_wraps_an_opaque_widget_in_types_that_declare_a_lifetime() {
    let (rawr, cargo) = both_printed("areas.rs", "rule,rule,Email,outside,Email");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("3 view pad 0 |Email| opaque area container 2 opaque rule 2")
            && rawr.contains("4 view pad 0 |me2@rawr| opaque area container 2 opaque rule 2")
            && rawr.ends_with("last view pad 0 |me0@rawr| opaque area container 2 opaque rule 0"),
        "{rawr}"
    );
}

/// iced's modal (#955, #1190) itself: as `areas.rs`, but the content the
/// opaque widget wraps borrows the view, so `opaque(content)` is made in
/// the view's region, and so are the container and mouse area that hold
/// it. Rust code can't name their types, so the region holds each boxed,
/// through a trait of the runner's implemented for the type's shape with a
/// parameter at the opaque position, whose methods are the calls the
/// program makes on it. The container's `style(|_theme| ..)` takes a script
/// closure, which the trait's method takes boxed, as the crate's `Fn` bound
/// takes it. The output is `cargo run`'s.
#[test]
fn a_script_function_wraps_opaque_widgets_in_types_that_declare_a_lifetime() {
    let (rawr, cargo) = both_printed("modal.rs", "Show,Email,outside,Show,Submit");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("3 view pad 0 container 10 pad 0 [Show] <x Shown:0> opaque area container 2 shade 80 opaque container 1 pad 0 |me1@rawr| [Submit]")
            && rawr.contains("5 message HideModal")
            && rawr.ends_with("last view container 10 pad 0 [Show] <Shown:0>"),
        "{rawr}"
    );
}

/// iced's component (#965, #1075): the view gives `container(opaque(..))
/// .padding(..)`, a `Container<'_, impl Widget, Theme>` of an opaque
/// widget a crate function made of script values, so no region holds it.
/// Rust code can't name its type, so the slot is generic over it, and the
/// witness of the opaque type, the output of the crate function's item at
/// its inputs, picks it where `run` takes the slot. The output is `cargo
/// run`'s.
#[test]
fn a_slot_gives_a_container_of_an_opaque_widget_made_of_script_values() {
    let (rawr, cargo) = both_printed("composed.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view container 0 opaque [Increment]")
            && rawr.ends_with("last view container 3 opaque [Increment]"),
        "{rawr}"
    );
}

/// iced's pane_grid (#1190): `pane(opaque(column![..]))` holds an opaque
/// widget made of the view's values, `Pane<'_, (), impl Widget>`, held
/// boxed in the view's region as modal's are, and its `title_bar` takes
/// another value of the region, a container of a field that borrows the
/// state's title. The output is `cargo run`'s.
#[test]
fn a_boxed_region_value_takes_another_value_of_its_region() {
    let (rawr, cargo) = both_printed("titled_pane.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with(
            "last view pane container 1 |pane 3| opaque pad 0 [Increment] 2@50 [Decrement]"
        ),
        "{rawr}"
    );
}

/// iced's pane_grid (#1191, #1082): `Panes::new(&self.board, |index,
/// tile, last| ..)` calls its closure while the view's region is open, and
/// the closure gives a `Pane<'a, ..>` at the panes' own lifetime,
/// invariant in it. The closure's call runs in a region of its own,
/// anchored on the pins of the region the call is made in, and its pane
/// comes out of it at that lifetime; its `title_bar` takes a container of
/// the same region. The output is `cargo run`'s.
#[test]
fn a_closure_gives_a_value_at_its_calls_lifetime_from_a_region_of_its_own() {
    let (rawr, cargo) = both_printed("board.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with(
            "last view panes 2 pane container 4 0@16 [Decrement] pane container 4 1@16 [Pane] pane container 4 2@16 [Pane] pane container 4 3@16 [Increment]"
        ),
        "{rawr}"
    );
}

/// As `board.rs`, but each pane's body is a crate's opaque widget, so the
/// closure gives `Pane<'a, T, impl Widget>` and `Panes::new` gives a type
/// that holds the opaque type it never takes. The output is `cargo run`'s.
#[test]
#[ignore = "blocked: `Panes::new` counts as the producer of the opaque type its closure gives, so its table compiles `title_bar` and `pane` at its own output's type (E0308 in `take_like`); new gap from #1191"]
fn a_closure_gives_a_value_holding_an_opaque_type_at_its_calls_lifetime() {
    let (rawr, cargo) = both_printed("board_opaque.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("pane container 4 3@16 opaque pad 0 [Increment] 30@16"),
        "{rawr}"
    );
}

/// iced's lazy (#328, #1081): `lazy(self.version, |_| ..)` is a widget only
/// where what its closure gives is `'static`, as its impl's `W: Widget +
/// 'static` says, and a container and `column!` take it. The widgets the
/// closure makes own what they show, so rustc names them at `'static` and
/// so does the runner, and the literal of each row's `button("Delete")`,
/// made while the widget lays out and outside the view's region, is a
/// `&'static str`. The output is `cargo run`'s.
#[test]
fn a_widget_whose_closure_gives_static_widgets_is_boxed_in_a_view() {
    let (rawr, cargo) = both_printed("lazy.rs", "Add,Add,Delete,Add");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("3 view pad 0 container 1 lazy pad 0 row 0 item2@16 [Delete] row 0 item3@16 [Delete] [Add]")
            && rawr.contains("3 message Delete(\"item2\")")
            && rawr.ends_with("last view pad 0 container 1 lazy pad 0 row 0 item2@16 [Delete] row 0 item3@16 [Delete] [Add]"),
        "{rawr}"
    );
}

/// iced's progress_bar: `transition(value, |at| ..)` keeps a closure that
/// makes its content each time it shows, as `view: Box<dyn Fn(..) -> W>`,
/// and the view's `column!` boxes it. A closure gives its widget from the
/// arena, at `'static`, so the region holds the transition with `W` at
/// `'static`, not at the view's lifetime (#1083). The output is `cargo
/// run`'s.
#[test]
fn a_widget_whose_closure_reads_the_state_is_boxed_in_a_view() {
    let (rawr, cargo) = both_printed("transition.rs", "Add,Add");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 transition 20@22 transition 2@16 [Add]"),
        "{rawr}"
    );
}

/// iced's progress_bar with a view closure that captures nothing (#1130):
/// native code lends the closure a crate value, `&Spring`, as
/// `transition`'s view takes `&Animation<f32>`, and the closure gives a
/// widget at the view's lifetime. The closure is a function native code
/// calls with the value it lends. The output is `cargo run`'s.
#[test]
fn a_view_function_lent_a_crate_value_is_boxed_in_a_view() {
    let (rawr, cargo) = both_printed("animated.rs", "Add,Add");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.ends_with("last view pad 0 animated 22@20 [Add]"),
        "{rawr}"
    );
}

/// iced's progress_bar (#1129): a transition made before an `if` moves
/// into a `row!` in one branch and a `column!` in the other, and each
/// branch's widget is spaced or padded, boxed and centered. Each region
/// value keeps its own family whichever container it joins. The output is
/// `cargo run`'s.
#[test]
fn a_region_value_moved_into_a_row_or_a_column_keeps_its_family() {
    let (rawr, cargo) = both_printed("oriented.rs", "Add,Flip,Add,Flip");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("row 2 transition 20@21 [Flip]")
            && rawr.ends_with("last view pad 0 pad 0 pad 3 transition 20@20 [Flip] [Add]"),
        "{rawr}"
    );
}

/// iced's pick_list: `column!` boxes a string literal through the crate's
/// impl of its trait for `&str`, and a choice takes an `Option` of a script
/// enum, a script `&[T]` constant as its `L: Borrow<[T]>`, and the enum's
/// `Display` as a callable, and compares options by the enum's derived
/// `PartialEq`. The output is `cargo run`'s.
#[test]
fn a_choice_of_script_values_beside_a_string_literal_runs() {
    let (rawr, cargo) = both_printed("pick.rs", "Ruby,Some other language");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "1 view pad 0 'Which is your favorite language?' \
             <Choose a language...: Elm|Ruby|Rust|Some other language>"
        ) && rawr.contains("1 message LanguageSelected(Ruby)")
            && rawr.contains("2 message LanguageSelected(Other)")
            && rawr.contains(
                "last view pad 0 'Which is your favorite language?' \
                 <Some other language: Elm|Ruby|Rust|*Some other language>"
            ),
        "{rawr}"
    );
}

/// iced's pokedex (#1632): `column!` boxes a `String`'s `as_ref()`, whose
/// `AsRef` target only the crate's `impl Widget for &str` fixes, as rustc
/// selects the one impl whose self type is a shared reference. The output
/// is `cargo run`'s.
#[test]
fn a_strings_as_ref_takes_its_target_from_the_widget_impl_it_boxes() {
    let (rawr, cargo) = both_printed("lent.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 message Increment") && rawr.contains("'pressed 1'"),
        "{rawr}"
    );
}

/// iced's solar_system and qr_code (#591): `pick(..)`'s `V: Borrow<T>` is a
/// reference to a crate enum the state holds, from `Some(&self.backend)`
/// and `self.chosen.as_ref()`, and a reference to a script enum, and its
/// `L` is a crate's `&'static [Self]` constant. The view borrows the crate
/// value, which `update` then replaces. The output is `cargo run`'s.
#[test]
fn a_reference_to_a_crate_or_script_value_fills_a_crate_type_argument() {
    let (rawr, cargo) = both_printed("selected.rs", "Software,Elm");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains(
            "1 view pad 0 <Best: *Best|Software> <Mine: Best|Software> <Rust: *Rust|Elm>"
        ) && rawr.ends_with(
            "last view pad 0 <Software: Best|*Software> <Mine: Best|Software> <Elm: Rust|*Elm>"
        ),
        "{rawr}"
    );
}

/// iced's qr_code (#590): `qr_code<'a, Theme>(data: &'a Data) ->
/// QRCode<'a, Theme>` borrows the state's data for the view's lifetime.
/// The data isn't `Clone` and counts its draws, so the count shows each
/// view borrows the one value, and `update` changes it in place once no
/// view borrows it. The output is `cargo run`'s.
#[test]
fn a_widget_borrows_a_crate_value_of_the_state_for_the_views_lifetime() {
    let (rawr, cargo) = both_printed("qr.rs", "Limit,Limit");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 <Limit:0> qr 9@10 draw 1")
            && rawr.contains("2 view pad 0 <x Limit:0> qr 11@16.923077 draw 2")
            && rawr.ends_with("last view pad 0 <Limit:0> qr 13@10 draw 3"),
        "{rawr}"
    );
}

/// iced's combo_box: `combo_box(&'a state, .., selection: Option<&T>, ..)`
/// lends its selection at a late-bound lifetime for the call alone. The
/// output is `cargo run`'s.
#[test]
fn an_option_of_a_late_bound_reference_is_lent_for_the_call() {
    let (rawr, cargo) = both_printed("chosen.rs", "Elm,Rust");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 <Type a language...? Rust|Elm>")
            && rawr.contains("2 view pad 0 <Elm? Rust|Elm>")
            && rawr.ends_with("last view pad 0 <Rust? Rust|Elm>"),
        "{rawr}"
    );
}

/// iced's changelog: `markdown::view<'a>(items: &'a [Item], ..) -> impl
/// Widget<Uri, ..> + 'a` borrows the state's items for the view's lifetime,
/// and `map` and `column!` take the opaque widget. The items aren't `Clone`
/// and count their draws, so the counts show each view borrows the one
/// value of each item, while `update` changes the rest of the state. The
/// output is `cargo run`'s.
#[test]
#[ignore = "blocked: `.map(..)` takes the borrowing value by value, and a lent cell's value moves only where it implements its one trait, without arguments, through its proxy (#856); `Framed<String, Theme, Renderer>` has arguments"]
fn an_opaque_widget_borrows_a_slice_of_crate_values_for_the_views_lifetime() {
    let (rawr, cargo) = both_printed("notes.rs", "first,Reset");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 [Reset] map md first@16 draw 1 md second@16 draw 1")
            && rawr.contains("1 message LinkClicked(\"first\")")
            && rawr.contains("2 view pad 0 [Reset] map md first@12 draw 2 md second@12 draw 2")
            && rawr.ends_with("last view pad 0 [Reset] map md first@16 draw 3 md second@16 draw 3"),
        "{rawr}"
    );
}

/// #846: `view<'a>(items: &'a [Item]) -> impl Show + 'a` keeps the items
/// borrowed for as long as its result lives, as rustc has it. The items are
/// read while it lives and pushed to once it drops, at `drop` and at the
/// end of a block. A result invariant in its lifetime (`Cell<&'a Item>`)
/// and one kept in a struct field across calls run too, and a program
/// struct lends `&self.items` (#848). An `Item` has no `Clone` and counts
/// each time it is shown, so the counts show every result borrowed the one
/// value of each item, which went back to its place after. The output is
/// `cargo run`'s.
#[test]
fn an_opaque_result_borrows_its_slice_for_as_long_as_it_lives() {
    let (rawr, cargo) = both_printed("shelf.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("picked <first> <second>")
            && rawr.contains("panel 2 [first second third more fourth]")
            && rawr.contains("shown first:6,second:6,third:3,more:2,fourth:2,fifth:0")
            && rawr.ends_with("counted first:8,second:8,third:5,more:4,fourth:4,fifth:2"),
        "{rawr}"
    );
}

/// #846: pushing to the items while a result that borrows them lives, after
/// its last use but before it drops, and moving them meanwhile, are rustc's
/// E0502 and E0505, which stop the program before anything runs, with
/// cargo's report alone.
#[test]
fn using_what_an_opaque_result_borrows_while_it_lives_is_rustcs_error() {
    let mutable = "cannot borrow `items` as mutable because it is also borrowed as immutable";
    for (variant, code, message, at) in [
        ("shelf_push.rs", "E0502", mutable, "main.rs:7:5"),
        ("shelf_dropped.rs", "E0502", mutable, "main.rs:8:5"),
        (
            "shelf_move.rs",
            "E0505",
            "cannot move out of `items` because it is borrowed",
            "main.rs:7:17",
        ),
    ] {
        let stderr = checked(variant);
        let cargo = cargo_s_report(&stderr);
        assert!(
            cargo.contains(&format!("error[{code}]: {message}")) && cargo.contains(at),
            "{variant}: {stderr}"
        );
    }
}

/// #849: a borrowing `impl Stocked<Item = String> + 'a` result of a trait
/// that can't be a trait object: a supertrait's method, a generic method at
/// two types, the associated type, the associated const through a method,
/// and a method that gives `Self`, whose result outlives the first and
/// borrows the same items. Each result says when it drops, and the items
/// count each show, so the output, which is `cargo run`'s, shows each
/// result dropped where rustc drops it and borrowed the one value of each
/// item.
#[test]
fn an_opaque_result_of_a_trait_that_cant_be_an_object_runs_through_its_proxy() {
    let (rawr, cargo) = both_printed("stock.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("tag n3 73")
            && rawr.contains("again 1[second third] second\ndrop 0\nstill 2[third]\ndrop 2")
            && rawr.ends_with("drop 1\npushed first:1,second:2,third:2,fourth:0"),
        "{rawr}"
    );
}

/// #849: a result that `again(&self) -> Self` gives borrows the items the
/// first borrowed, after the first drops: pushing to them while it lives is
/// rustc's E0502, which stops the program before anything runs, with
/// cargo's report alone.
#[test]
fn a_result_a_method_gives_as_self_keeps_the_items_borrowed() {
    let stderr = checked("stock_again.rs");
    let cargo = cargo_s_report(&stderr);
    assert!(
        cargo.contains(
            "error[E0502]: cannot borrow `items` as mutable because it is also borrowed as immutable"
        ) && cargo.contains("main.rs:10:5"),
        "{stderr}"
    );
}

/// #856: a crate function that takes a borrowing result by value,
/// `wrap(shown: impl Show) -> impl Show`, gives a result that keeps the
/// items borrowed for as long as it lives, as rustc has it: wrapped once,
/// wrapped again, moved out of a binding, and in a block. The counts show
/// every show read the one value of each item. The output is `cargo run`'s.
#[test]
fn a_crate_call_takes_a_borrowing_result_by_value() {
    let (rawr, cargo) = both_printed("shelf_wrap.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("twice (([first second]))")
            && rawr.contains("scoped (<first>) (<second>)")
            && rawr.ends_with("counted first:4,second:4,third:1,fourth:0,fifth:0"),
        "{rawr}"
    );
}

/// #856: pushing to the items while a wrapped result that borrows them
/// lives is rustc's E0502, which stops the program before anything runs,
/// with cargo's report alone.
#[test]
fn using_what_a_wrapped_result_borrows_while_it_lives_is_rustcs_error() {
    let stderr = checked("shelf_wrap_push.rs");
    let cargo = cargo_s_report(&stderr);
    assert!(
        cargo.contains(
            "error[E0502]: cannot borrow `items` as mutable because it is also borrowed as immutable"
        ) && cargo.contains("main.rs:7:5"),
        "{stderr}"
    );
}

/// #851: a function in a module nested under a module the program imports,
/// `b::c::f` after `use events::nested::b`, at any depth and through a
/// re-export of the module, as iced's `operation::scrollable::snap_to` is
/// reached. The output is `cargo run`'s.
#[test]
fn a_function_in_a_module_nested_under_an_imported_module_runs() {
    let (rawr, cargo) = both_printed("nested.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "f 2\ng deep\nthrough 42\ndeeper deep\ncount 10 11");
}

/// `rawr-import check` of a lane of the fixture with `variant` as its
/// `main.rs`, which fails: its stderr.
fn checked(variant: &str) -> String {
    checked_in(&lane(Some(variant)))
}

/// `rawr-import check` of a lane of the fixture with `variant` as its
/// `main.rs`, which passes.
fn passes_check(variant: &str) {
    let root = lane(Some(variant));
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
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

/// `rawr-import check` of a lane of the fixture with `variant` as its
/// `main.rs`, which fails with rustc's `code`, as `cargo build` of the
/// same lane does: rawr's stderr.
fn rejected_as_cargo(variant: &str, code: &str) -> String {
    let root = lane(Some(variant));
    let stderr = checked_in(&root);
    let build = Command::new("cargo")
        .args(["build", "-q"])
        .current_dir(&*root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    let rustc = String::from_utf8_lossy(&build.stderr);
    let error = format!("error[{code}]");
    assert!(!build.status.success() && rustc.contains(&error), "{rustc}");
    assert!(stderr.contains(&error), "{stderr}\nrustc:\n{rustc}");
    stderr
}

/// `rawr-import check` of the project at `root`, as `checked` gives it.
fn checked_in(root: &std::path::Path) -> String {
    let directory = crate::support::scratch();
    let check = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("check")
        .arg(root)
        .env("RAWR_HOME", directory.join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&check.stderr).into_owned();
    assert!(!check.status.success(), "{stderr}");
    stderr
}

/// iced's canvas examples: the program's own type implements the crate's
/// `canvas::Program`, and `canvas(self)` takes the view's `&self`. The
/// crate calls the impl's `draw` back while it lays the view out, lending
/// it a pen and a theme whose methods it calls, and its `update` while it
/// handles a press, with a script state lent `&mut` and an event it
/// matches. A script color crosses into the crate's `impl Into<Color>`
/// through the script's `impl From<Shade> for Color`, as lazy's does. The
/// output is `cargo run`'s.
#[test]
fn a_script_impl_of_a_crate_trait_is_called_back_by_the_crate() {
    let (rawr, cargo) = both_printed("program.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view canvas[primary plain 0#1, primary 40x30 after 0 rgb(10, 20, 30)#2]")
            && rawr.contains(
                "pressed Increment: canvas[primary plain 0#1, primary 40x30 after 1 rgb(10, 20, 30)#2]"
            )
            && rawr.contains("1 message Pressed(\"Increment\")")
            && rawr.contains(
                "last view canvas[primary plain 36#1, primary 40x30 after 0 rgb(10, 20, 30)#2]"
            ),
        "{rawr}"
    );
}

#[test]
fn native_trait_callbacks_preserve_builtin_result_signatures_and_variants() {
    let (rawr, cargo) = both_printed("results.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "check Ok(()) Err(\"check\")\nvalue Ok(17) Err(\"value\")\necho Ok(23) Err(\"input\")"
    );
}

/// A crate's `-> !` functions cross: a call of one is a value of whatever
/// type its place takes, in an `if` or `match` arm and a `let .. else`,
/// and `finish` ends the process with its code, as `std::process::exit`
/// does. The output is `cargo run`'s.
#[test]
fn a_crate_function_returning_never_is_a_value_of_any_type() {
    let (rawr, cargo) = both_printed("diverging.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "3\n2 5\nfinishing");
}

/// A match binds a crate variant's fields without `Clone` as rustc does
/// (#678): by value, also below a `Result`'s `Err`, it moves a `Door` and
/// an `io::Error` out, so a consuming method and a by-value call take
/// them, and a guard reads them by reference first; through a reference it
/// lends a `Door`, so `&self` methods and `&T` parameters reach it while
/// the arm reads the matched value too, and a test of the variant reads no
/// field. Each field is read alone (#785): a pattern below a variant's
/// tests its field lent to it and moves or lends what it binds, and a
/// `Copy` field is copied beside a field without `Clone`. Through `&mut`
/// and as `ref mut` it lends a `Door` mutably (#786), so a `&mut self`
/// method changes the matched value. A field bound through a reference is
/// borrowed where it is, a `Clone` one too (#782), so a crate function sees
/// the very field the value holds, and the borrow outlives its arm (#783):
/// a function returns it, a `match` gives it to a `let`, and a `let`-`else`
/// keeps it for the statements after; `.clone()` of it is a value of its
/// own. The output is `cargo run`'s.
#[test]
fn a_match_moves_or_lends_variant_fields_without_clone() {
    let (rawr, cargo) = both_printed("matched.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, MATCHED);
}

/// `let Press::Down(door) = ..;` on a crate enum of that one variant
/// destructures it, as rustc does (#1681), while the same `let` on one
/// marked `#[non_exhaustive]` is E0005, as the driver reads the marker.
/// The output is `cargo run`'s.
#[test]
fn a_let_on_a_crate_enums_only_variant_runs_unless_non_exhaustive() {
    let (rawr, cargo) = both_printed("pressed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(rawr, "down closed gate\nheld vent");
    let stderr = rejected_as_cargo("pressed_sealed.rs", "E0005");
    assert!(stderr.contains("pattern `_` not covered"), "{stderr}");
}

/// What `matched.rs` prints, as `cargo run` does.
const MATCHED: &str = "ok 4\nclosed vent\nfailed\n\
                       door door door open\nclosed door\nfailed\n3 consumed refused\n\
                       gate closed gate\nclosed hatch\nidle\n\
                       below closed cellar\ntried 4 attempts\nquiet\n2 counted\n\
                       above door attic\nagain failed\nbarn 5\n6\n7 by value\n\
                       deck door deck open\nstoop door stoop open\nclosed terrace\nbarn 3\n\
                       kept true\ndoor kept open\nloft door loft\n\
                       Tag { label: \"blue\" } blue true\nblue true\nblue true false\ntrue";

/// #893: a program runs in a project as it would in a fresh one after
/// another program ran there. The first program's `column!` reaches the
/// crate through its macros' `$crate`, so the project's remembered demand
/// keeps the crate's types at their hidden paths too. The next program's
/// values of those types are the types a crate signature or adapter names
/// at their public paths, as rustc has them, and a crate method that gives
/// an opaque receiver's own type keeps what the receiver keeps.
#[test]
fn a_program_runs_as_it_would_fresh_after_another_ran_in_its_project() {
    let root = lane(None);
    assert_eq!(
        printed_in(&root),
        format!("title crossing::Counter\n{EVENTS}")
    );
    let replace = |variant: &str| {
        fs::write(
            root.join("src/main.rs"),
            CROSSING.read(&format!("variants/{variant}")),
        )
        .unwrap();
    };
    replace("matched.rs");
    assert_eq!(printed_in(&root), MATCHED);
    replace("stock_again.rs");
    let stderr = checked_in(&root);
    assert!(
        cargo_s_report(&stderr).contains(
            "error[E0502]: cannot borrow `items` as mutable because it is also borrowed as immutable"
        ),
        "{stderr}"
    );
}

/// iced's `keyboard::Key<C = SmolStr>`: an enum whose every type parameter
/// has a default is matched below a variant that names it without
/// arguments, as modal's `KeyPressed { key: Key::Named(Named::Tab), .. }`,
/// and made at the instance its use infers, its default where nothing else
/// fixes it. The output is `cargo run`'s.
#[test]
fn an_enum_with_defaulted_parameters_is_matched_and_made() {
    let (rawr, cargo) = both_printed("keys.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "back\nforward\nclose\ntyped a\nother\nother\nNamed(Escape)"
    );
}

/// iced's layout: a crate closure takes a key press whose fields hold an
/// `Option` of the key's defaulted text type, destructured by value with a
/// literal field and `..`. Each crate value below a variant's fields, an
/// `Option`'s too, has its own type (#971). The output is `cargo run`'s.
#[test]
fn a_variants_optional_crate_field_has_its_own_type() {
    let (rawr, cargo) = both_printed("typed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "[\"character A\", \"escape\"]\n[Some(\"A\"), None, None]"
    );
}

/// iced's layout: each example is a `fn() -> Element<'static, Message>` in
/// a constant table, called through its pointer by the view. One makes a
/// `column![..]`, sets its padding and boxes it, as layout's `column_`
/// does (#971). The output is `cargo run`'s.
#[test]
fn a_column_an_example_pointer_boxes_is_shown() {
    let (rawr, cargo) = both_printed("examples.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.contains("pad 40 A column can be used to@"), "{rawr}");
}

/// iced's bezier_tool: a `Copy` cursor's by-value method gives an `Option`
/// of a point whose coordinate type has a default, read directly, through
/// `?` and in a closure native code calls with the cursor. The point has
/// the output's type. The output is `cargo run`'s.
#[test]
fn a_by_value_methods_optional_crate_output_has_its_type() {
    let (rawr, cargo) = both_printed("cursor.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "5 10\nSome(Point { x: 10.0, y: 10.0 })\nNone\nSome(15.0)"
    );
}

/// String-like generics follow their bound, not their spelling (#713): `S:
/// Into<String>` takes what `impl Into<String>` takes, and `AsRef<str>`,
/// `AsRef<Path>`, `Into<PathBuf>`, `AsRef<[u8]>` and `ToString` take a
/// literal, a `String`, a borrow of one, a `PathBuf` and a borrow of one,
/// as std's impls for them say. The output is `cargo run`'s.
#[test]
fn string_like_generics_take_what_their_bounds_impls_take() {
    let (rawr, cargo) = both_printed("named.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("spelled Grace\nspelled Ada\n5 4 4\nopened icon.png in assets\nopened inter.ttf in assets/fonts"),
        "{rawr}"
    );
}

/// A crate generic's parameter that appears only inside an argument's
/// bound, as `L` of `rich(impl AsRef<[Span<'a, L>]>)`, is the one std's or
/// the crate's impl of the bound for the argument's type gives: a `Vec`, an
/// array, a slice, through `AsRef`, `Borrow`, `IntoIterator` and a crate
/// trait, in a function and a method; and a function a `use` names takes a
/// turbofish, as `span::<()>("e")`. The output is `cargo run`'s.
#[test]
fn a_crate_generics_parameter_is_inferred_through_an_arguments_bound() {
    let (rawr, cargo) = both_printed("spans.rs", "");
    assert_eq!(rawr, cargo);
    assert!(rawr.contains("rich a|b at 13\nrich c|d at 16"), "{rawr}");
}

/// image's `ImageBuffer::from_fn(w, h, |x, y| ..)`: an `FnMut` with no
/// `'static` bound is borrowed for the call, so the closure changes what
/// it captures and the program reads the change after, and function
/// pointers of none, two and three inputs take script functions (#715).
/// The output is `cargo run`'s.
#[test]
fn a_non_static_fn_mut_and_function_pointers_of_any_arity_run() {
    let (rawr, cargo) = both_printed("grid.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "4x3 after 12 calls\n0 31 22\n12 seen, 64 at (3, 2)\nsum 384\n123 7"
    );
}

#[test]
fn native_sequence_callbacks_retain_mutations_ownership_and_unwind_history() {
    let (rawr, cargo) = both_printed("sequences.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("local len=1 drops=0 capacity=8 allocation=true"),
        "{rawr}"
    );
    assert!(rawr.contains("local final-drops=1"), "{rawr}");
    assert!(
        rawr.contains(
            "counted mode=0 values=[1, 2, 3] clones=0 drops=0 capacity=6 allocation=true"
        ),
        "{rawr}"
    );
    assert!(
        rawr.contains("counted mode=3 values=[1, 2, 3] clones=1 drops=1"),
        "{rawr}"
    );
    assert!(
        rawr.contains("slot values=[\"entry-slot\", \"slot call\"] cap=12 vector=true string=true"),
        "{rawr}"
    );
    assert!(
        rawr.contains(
            "mode=8 values=[\"native-native\", \"native call\"] cap=16 vector=true string=true"
        ),
        "{rawr}"
    );
    assert!(
        rawr.contains(
            "closure values=[\"entry-closure\", \"closure call\"] cap=12 vector=true string=true"
        ),
        "{rawr}"
    );
}

/// An `Rc` or `Arc` of a crate's type crosses as the crate's own pointer
/// (#775): one the crate gives keeps the copy the crate holds in its count
/// and is `ptr_eq` to another of the same allocation; an `Arc` taken by
/// value sees rawr's other pointers in its count; one passed and given
/// back is the same pointer; and one the program makes with `Arc::new`
/// becomes the crate's pointer the first time it crosses. Its value's
/// methods run through `Deref`. The output is `cargo run`'s.
#[test]
fn an_arc_of_a_crate_value_crosses_as_the_crates_own_pointer() {
    let (rawr, cargo) = both_printed("pointers.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "shared 3 true shared\nmade 2 true\nkept true 2 2\nlevel 2\n\
         after 1 Config { name: \"made\", level: 2 }\nown true 2\nheld 3\n\
         back true 2 own\nnode leaf node leaf\nholders 3\ndropped 1"
    );
}

/// std's maps and sets in a crate's signature are rawr's (#777): a
/// `&HashMap` and a `&BTreeMap` of crate values without `Clone` lend each
/// value to the call and keep it after, a `&HashSet` of strings and a
/// `HashMap` by value are made from rawr's, and a `BTreeMap`, a `HashMap`
/// of crate values, a `BTreeSet` and a `HashSet` given back are rawr's,
/// in key order. The output is `cargo run`'s.
#[test]
fn maps_and_sets_cross_as_rawrs_keyed_collections() {
    let (rawr, cargo) = both_printed("keyed.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "total 7\nkept 3 2\ncheapest nib nib\nprices {\"a\": 1, \"b\": 2}\n\
         catalog [\"cup\", \"mug\"] 5\ntags 8\nevens {0, 2, 4, 6}\n\
         letters ['a', 'b', 'c']\ninvert {1: \"one\", 2: \"two\"}"
    );
}

/// A crate type's derived `Ord` keys a `BTreeMap` in the crate's order,
/// not its hash's (#363), and a method that gives the same enum at a type
/// borrowing its receiver, as iced's `Key::as_ref(&self) -> Key<&str>`,
/// is matched at that type, a `&str` literal against the borrowed
/// character (#345). The output is `cargo run`'s.
#[test]
fn a_crate_order_keys_a_btree_map_and_a_borrowed_instance_matches() {
    let (rawr, cargo) = both_printed("ordered.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "windows [\"third\", \"second\", \"first\"]\nfirst Id(8) third\n\
         Some(\"toggle\")\nSome(\"reset\")\nother q\nNone\nNone\nNone"
    );
}

/// std's types in a crate's signature are its own in every position (D711
/// step 5, #707): a tuple taken and given, `&String`, `&Vec<T>`,
/// `Range<usize>` taken and given, `&Path` and a `Box<i64>` taken, given
/// and passed back. A crate type's std trait impls are demanded like its
/// own trait's (#709): `Index` through `grid[2]`, and `FromStr` through
/// `.parse::<Coords>()` and a `parse()` whose target a later annotation
/// fixes. The output is `cargo run`'s.
#[test]
fn std_types_cross_in_every_position_and_std_trait_impls_are_demanded() {
    let (rawr, cargo) = both_printed("signatures.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "area 12\nswap (\"one\", 1)\nshout HI\ncount 2 total 6\nspan 5 halves 0..5\n\
         stem file\nboxed 8 unbox 6\nrebox 9\n\
         parsed Coords { x: 1, y: 2 } 3 false\nrefused Err(\"no comma in 12\")\ngrid 20 30"
    );
}

/// An edit to the body of a script impl's function, which native code calls
/// through the proxy, takes effect at its next call, in the same process
/// with the state kept.
#[test]
fn an_edit_to_a_script_impl_takes_effect_at_its_next_call() {
    let root = lane(Some("program.rs"));
    let main = root.join("src/main.rs");
    let mut run = start(
        &root,
        &[
            ("EVENTS_COUNT", PathBuf::from("3000")),
            ("EVENTS_STOP", root.join("stop")),
        ],
    );
    let pid = run.until(|line| line.starts_with("pid "));
    run.until(|line| line.starts_with("3 view canvas[primary plain 18#1"));

    let text = fs::read_to_string(&main).unwrap();
    let written = "pen.stroke(&format!(\"{} {}\", theme.name(), self.presses)),";
    assert!(text.contains(written), "{text}");
    fs::write(
        &main,
        text.replace(
            written,
            "pen.stroke(&format!(\"{} {} edited\", theme.name(), self.presses)),",
        ),
    )
    .unwrap();
    // The presses go on from where they were: the state was kept.
    let edited = run.until(|line| line.contains(" edited#1"));
    let presses = edited
        .split("plain ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|presses| presses.parse::<i64>().ok())
        .unwrap_or_else(|| panic!("{edited}"));
    assert!(presses > 18, "{}", run.seen.join("\n"));
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
    assert_eq!(
        run.seen
            .iter()
            .filter(|line| line.starts_with("pid "))
            .count(),
        1,
        "{seen}"
    );
    assert!(run.seen.contains(&pid));

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// A save that changes a script impl's shape, here a function it adds to
/// override the trait's provided `update` with no native use of its own,
/// reaches native code: it calls the new function, not the provided one
/// the running build was written with (#326).
#[test]
fn an_edit_to_a_script_impls_shape_reaches_native_code() {
    let program = CROSSING.read("variants/program.rs");
    let update = "    fn update(&self, state: &mut Self::State, event: &Event, bounds: Bounds) -> Option<Message> {\n        state.count += 1;\n        match event {\n            Event::Pressed(label) if bounds.area() > 0 => Some(Message::Pressed(label.clone())),\n            Event::Pressed(_) => None,\n        }\n    }\n\n";
    assert!(program.contains(update), "{program}");
    let root = CROSSING.lease(&[("src/main.rs", &program.replace(update, ""))]);
    let main = root.join("src/main.rs");
    let mut run = start(
        &root,
        &[
            ("EVENTS_COUNT", PathBuf::from("600")),
            ("EVENTS_STOP", root.join("stop")),
        ],
    );
    // The presses a `N view canvas[primary plain P#..` line shows.
    let presses = |line: &str| {
        line.split_once(" view canvas[primary plain ")
            .and_then(|(_, rest)| rest.split(['#', ' ']).next())
            .and_then(|presses| presses.parse::<i64>().ok())
    };
    let before = run.until(|line| line.starts_with("3 view canvas["));
    assert_eq!(presses(&before), Some(0), "{}", run.seen.join("\n"));

    // Each press now gives a message of five letters, through no native
    // use the running build lacks.
    let pressing = "    fn update(&self, state: &mut Self::State, _event: &Event, _bounds: Bounds) -> Option<Message> {\n        state.count += 1;\n        Some(Message::Pressed(\"press\".to_owned()))\n    }\n\n";
    fs::write(&main, program.replace(update, pressing)).unwrap();
    let pressed = run.until(|line| presses(line).is_some_and(|presses| presses > 0));
    assert!(presses(&pressed) > Some(0), "{}", run.seen.join("\n"));

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}

/// `event` and the count a `N view count X` line shows.
fn parse(line: &str) -> Option<(i64, i64)> {
    let (event, count) = line.split_once(" view count ")?;
    Some((event.trim().parse().ok()?, count.trim().parse().ok()?))
}

#[test]
fn a_body_edit_takes_effect_at_the_next_event_with_the_state_kept() {
    let root = lane(None);
    let main = root.join("src/main.rs");
    let mut run = start(
        &root,
        &[
            ("EVENTS_COUNT", PathBuf::from("3000")),
            ("EVENTS_STOP", root.join("stop")),
        ],
    );
    let pid = run.until(|line| line.starts_with("pid "));
    let before = run.until(|line| parse(line).is_some_and(|(_, count)| count >= 3));
    let (_, kept) = parse(&before).unwrap();

    let text = fs::read_to_string(&main).unwrap();
    assert!(text.contains("counter.value += 1;"), "{text}");
    fs::write(
        &main,
        text.replace("counter.value += 1;", "counter.value += 10;"),
    )
    .unwrap();
    // The next events count by ten from where the state was.
    let previous = std::cell::Cell::new(None::<i64>);
    run.until(|line| {
        let Some((_, count)) = parse(line) else {
            return false;
        };
        let stepped = previous
            .get()
            .is_some_and(|previous| count - previous == 10);
        previous.set(Some(count));
        stepped
    });
    let counts = run
        .seen
        .iter()
        .filter_map(|line| parse(line))
        .map(|(_, count)| count)
        .collect::<Vec<_>>();
    let jump = counts
        .windows(2)
        .position(|pair| pair[1] - pair[0] == 10)
        .unwrap_or_else(|| panic!("{}", run.seen.join("\n")));
    // The state was kept: the count went on from where it was.
    assert!(counts[jump] >= kept, "{}", run.seen.join("\n"));
    assert!(
        counts[..=jump]
            .windows(2)
            .all(|pair| pair[1] - pair[0] == 1),
        "{}",
        run.seen.join("\n")
    );
    let seen = run.seen.join("\n");
    assert!(!seen.contains("restarting"), "{seen}");
    assert_eq!(
        run.seen
            .iter()
            .filter(|line| line.starts_with("pid "))
            .count(),
        1,
        "{seen}"
    );
    assert!(run.seen.contains(&pid));

    fs::write(root.join("stop"), "").unwrap();
    let status = run.child.wait().unwrap();
    assert!(status.success(), "{}", run.seen.join("\n"));
}
