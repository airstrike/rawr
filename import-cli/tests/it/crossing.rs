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
/// the slider's `u8`. The output is `cargo run`'s.
#[test]
fn a_slider_beside_an_opaque_widget_runs() {
    let (rawr, cargo) = both_printed("crossing-slider", "slider.rs", "slide");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 message SliderChanged(100)")
            && rawr.contains("last view pad 0 1..=100@100/Some(50) logo100@16"),
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
/// function, as a `text` function of the same name sits beside it. The
/// output is `cargo run`'s.
#[test]
fn a_macro_reaches_an_associated_function_through_an_alias() {
    let (rawr, cargo) = both_printed("crossing-text", "text.rs", "");
    assert_eq!(rawr, cargo);
    assert!(
        rawr.contains("1 view pad 20 [Increment] count 0 of 3@50 [Decrement]")
            && rawr.contains("last view pad 20 [Increment] count 2 of 3@50 [Decrement]"),
        "{rawr}"
    );
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
