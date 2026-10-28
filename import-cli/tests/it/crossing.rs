//! A script's own types and functions cross into a native generic entry
//! point in the shape of a GUI library's `run` (D594): `run::<S, M, U, V>`
//! makes the state with the script type's `Default`, names it by
//! `type_name`, and calls the script's `update(&mut S, M)` and its
//! higher-ranked `view(&S) -> Widget<'_, M>`, whose label borrows the
//! state's string. The output is `cargo run`'s, and a body edit takes
//! effect at the next event with the state kept. Build the driver first,
//! as for `plain`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::support::{copy, start};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crossing");

/// The fixture copied to its own root `name` under the test directory.
fn root(name: &str) -> PathBuf {
    let driver = Path::new(env!("CARGO_BIN_EXE_rawr-import")).with_file_name(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        driver.is_file(),
        "build {} first: cargo build -p rawr-import --features rustc-driver --bin rawr-import-driver",
        driver.display()
    );
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_file(root.join("stop"));
    let _ = fs::remove_dir_all(root.join("src"));
    copy(Path::new(FIXTURE), &root);
    root
}

/// `rawr-import run` on the fixture copied to `name`, with `variant` from
/// `variants/` as its `main.rs` when given: its stdout without the process
/// id, which `cargo run` of the same crate prints too.
fn printed(name: &str, variant: Option<&str>) -> String {
    let root = root(name);
    if let Some(variant) = variant {
        fs::copy(
            root.join("variants").join(variant),
            root.join("src/main.rs"),
        )
        .unwrap();
    }
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&root)
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
    assert_eq!(
        printed("crossing", None),
        format!("title crossing::Counter\n{EVENTS}")
    );
}

/// iced's shape: `application(Counter::update, Counter::view)` takes the
/// methods through traits implemented for every `Fn` of their shape, and
/// the theme and renderer are the ones the view's widget names.
#[test]
fn methods_cross_through_traits_over_fn_as_iced_takes_them() {
    assert_eq!(
        printed("crossing-application", Some("application.rs")),
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
        printed("crossing-counter", Some("counter.rs")),
        "title crossing::Counter\n\
         renderer events::widgets::fallback::Renderer<events::widgets::backend::Gpu, events::widgets::backend::Soft>\n\
         1 view pad 20 [Increment] 0@50 [Decrement]\n1 message Increment\n\
         2 view pad 20 [Increment] 1@50 [Decrement]\n2 message Increment\n\
         3 view pad 20 [Increment] 2@50 [Decrement]\n3 message Increment\n\
         4 view pad 20 [Increment] 3@50 [Decrement]\n4 message Decrement\n\
         last view pad 20 [Increment] 2@50 [Decrement]"
    );
}

/// `rawr-import run` and `cargo run` of the fixture copied to `name` with
/// `variant` as its `main.rs`, pressing the labels of `presses` in turn:
/// both programs' stdout without the process id.
fn both_printed(name: &str, variant: &str, presses: &str) -> (String, String) {
    let root = root(name);
    fs::copy(
        root.join("variants").join(variant),
        root.join("src/main.rs"),
    )
    .unwrap();
    let directory = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let output = |mut command: Command| {
        let run = command
            .current_dir(&root)
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
        .arg(&root)
        .env("RAWR_HOME", directory.join("rawr-home"));
    let mut cargo = Command::new("cargo");
    cargo.args(["run", "-q"]);
    (output(rawr), output(cargo))
}

/// iced's checkbox: `application(State::default, ..)` gives an opaque
/// `impl Program` that `fonts([..])` and `run()` take, `on_toggle_maybe`
/// takes an `Option` of a variant's constructor, and `style` takes a crate
/// function of two inputs. A press toggles; the second toggle only takes
/// presses once the first is checked. The output is `cargo run`'s.
#[test]
fn a_checkbox_with_an_optional_callable_and_a_crate_style_runs() {
    let (rawr, cargo) = both_printed("crossing-checkbox", "checkbox.rs", "Styled,Default,Styled");
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
    let (rawr, cargo) = both_printed("crossing-slider", "slider.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 0 1..=100@50/Some(50) \"50\" logo50@16")
            && rawr.contains("1 message SliderChanged(100)")
            && rawr.contains("last view pad 0 1..=100@100/Some(50) \"100\" logo100@16"),
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
    let (rawr, cargo) = both_printed("crossing-styling", "styling.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("last view pad 0 \"2 looks\"/mono0 \"mono0\" \"0\"/plain0"),
        "{rawr}"
    );
}

/// iced's tooltip: a derived `Default` builds a field of a crate enum by the
/// crate's own `Default`, a `match` reads it, and a box of a string hint
/// takes a crate function `fn(&Theme) -> Style` as its style. The output
/// is `cargo run`'s.
#[test]
fn a_tooltip_with_a_crate_default_field_and_a_crate_style_runs() {
    let (rawr, cargo) = both_printed("crossing-tooltip", "tooltip.rs", "Delay,Delay");
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
/// lazy iterator fills a column's `impl IntoIterator`. The output is
/// `cargo run`'s.
#[test]
fn a_macro_reaches_an_associated_function_through_an_alias() {
    let (rawr, cargo) = both_printed("crossing-text", "text.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 20 [Increment] count 0 of 3@50 pad 0 0+1@16 0+2@16 [Decrement]")
            && rawr.contains(
                "last view pad 20 [Increment] count 2 of 3@50 pad 0 2+1@16 2+2@16 [Decrement]"
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
    let (rawr, cargo) = both_printed("crossing-subscription", "subscription.rs", "Listen");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("heard Ticked(3)")
            && rawr.contains("1 view pad 0 <Listen:0> 'tick 3 of 6'@48")
            && rawr.contains("1 message Toggled(true)")
            && rawr.contains("last view pad 0 <x Listen:0> 'tick 3 of 6'@48"),
        "{rawr}"
    );
}

/// iced's canvas aliases: `canvas::Cache` passes its parameter on to a
/// type of another crate that no path of the root reaches, and defaults it
/// to the root's `Renderer` alias. A derived `Default` builds a field of it,
/// and its associated functions and methods run. `canvas::Geometry` names
/// an associated type of that renderer, at its default a type of its own,
/// and `Note<'a>` passes a lifetime and a defaulted theme on, as iced's
/// `widget::Text` does. The output is `cargo run`'s.
#[test]
fn aliases_that_default_to_an_alias_or_name_an_associated_type_cross() {
    let (rawr, cargo) = both_printed("crossing-canvas", "canvas.rs", "");
    assert_eq!(rawr, cargo);
    assert_eq!(
        rawr,
        "sketch circle,line\ncleared []\nboard frame\nprimary arc\nprimary hand\nnote 3"
    );
}

/// iced's pick_list: `column!` boxes a string literal through the crate's
/// impl of its trait for `&str`, and a choice takes an `Option` of a script
/// enum, a script `&[T]` constant as its `L: Borrow<[T]>`, and the enum's
/// `Display` as a callable, and compares options by the enum's derived
/// `PartialEq`. The output is `cargo run`'s.
#[test]
fn a_choice_of_script_values_beside_a_string_literal_runs() {
    let (rawr, cargo) = both_printed("crossing-pick", "pick.rs", "Ruby,Some other language");
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
    let (rawr, cargo) = both_printed("crossing-program", "program.rs", "");
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

/// An edit to the body of a script impl's function, which native code calls
/// through the proxy, takes effect at its next call, in the same process
/// with the state kept.
#[test]
fn an_edit_to_a_script_impl_takes_effect_at_its_next_call() {
    let root = root("crossing-program-swap");
    fs::copy(
        root.join("variants").join("program.rs"),
        root.join("src/main.rs"),
    )
    .unwrap();
    let main = root.join("src/main.rs");
    let mut run = start(
        &root,
        &[
            ("EVENTS_COUNT", PathBuf::from("3000")),
            ("EVENTS_STOP", root.join("stop")),
        ],
    );
    let pid = run.until(Duration::from_secs(900), |line| line.starts_with("pid "));
    run.until(Duration::from_secs(60), |line| {
        line.starts_with("3 view canvas[primary plain 18#1")
    });

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
    let edited = run.until(Duration::from_secs(120), |line| line.contains(" edited#1"));
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

/// `event` and the count a `N view count X` line shows.
fn parse(line: &str) -> Option<(i64, i64)> {
    let (event, count) = line.split_once(" view count ")?;
    Some((event.trim().parse().ok()?, count.trim().parse().ok()?))
}

#[test]
fn a_body_edit_takes_effect_at_the_next_event_with_the_state_kept() {
    let root = root("crossing-swap");
    let main = root.join("src/main.rs");
    let mut run = start(
        &root,
        &[
            ("EVENTS_COUNT", PathBuf::from("3000")),
            ("EVENTS_STOP", root.join("stop")),
        ],
    );
    let pid = run.until(Duration::from_secs(900), |line| line.starts_with("pid "));
    let before = run.until(Duration::from_secs(60), |line| {
        parse(line).is_some_and(|(_, count)| count >= 3)
    });
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
    run.until(Duration::from_secs(120), |line| {
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
