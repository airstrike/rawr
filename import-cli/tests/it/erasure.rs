//! A script type a native generic fills keeps its type (D513, review 1, 2,
//! 3, 8 and 9): it crosses as its own Rust type (D594), so a native
//! `TypeId` cast to another script type, or to another instance of a
//! generic one, gives `None` as in Rust, a closure native code feeds is fed
//! only its own type, a `Clone` bound is checked on the script type, native
//! `Clone` and `Debug` are the script type's own, and a value holding a
//! native one passes through whole. The generated runner's own runtime
//! tests run in the runner the press test generates. Build the driver
//! first, as for `plain`.

use std::process::Command;

use crate::support::fixture::Lease;
use crate::support::Fixture;

static ERASURE: Fixture = Fixture::new("erasure");

/// `rawr-import run` on a lane of the `erasure` fixture with `main` as its
/// program: the lane, whether it succeeded, its stdout and its stderr.
fn run(main: &str) -> (Lease, bool, String, String) {
    let root = ERASURE.lease(&[("src/main.rs", &format!("// rawr\n{main}"))]);
    let run = Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .arg("run")
        .arg(&*root)
        .env("RAWR_HOME", crate::support::scratch().join("rawr-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    (
        root,
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

/// A case that must run and print `expected`: the lane it ran in.
fn runs(main: &str, expected: &str) -> Lease {
    let (root, success, stdout, stderr) = run(main);
    assert!(success, "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout, expected, "{main}\nstderr:\n{stderr}");
    root
}

/// A case that must stop, saying `said` and printing nothing.
fn refused(main: &str, said: &str) {
    let (_, success, stdout, stderr) = run(main);
    assert!(!success, "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        !stdout.contains("wrong"),
        "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains(said),
        "{main}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

const TYPES: &str = "
#[derive(Debug, Clone)]
struct Envelope<T> {
    value: T,
}

struct Ticket {
    number: i64,
}

struct Order {
    count: i64,
}
";

/// Review 1: a script type crosses as its own Rust type (D594), so a
/// `TypeId` cast to the same instance works and one to another instance
/// of the generic, or another script type, gives `None`, as in Rust. An
/// unsuffixed literal is an `i32` there, as rustc infers it.
#[test]
fn a_script_type_filling_a_native_generic_keeps_its_type() {
    runs(
        &format!(
            "{TYPES}
fn main() {{
    let same: Option<Envelope<i64>> = natives::cast(Envelope {{ value: 1_i64 }});
    println!(\"same {{}}\", same.map(|envelope| envelope.value).unwrap_or(-1));
    let literal: Option<Envelope<i64>> = natives::cast(Envelope {{ value: 1 }});
    println!(\"literal {{}}\", literal.is_some());
    let other: Option<Envelope<String>> = natives::cast(Envelope {{ value: 1_i64 }});
    println!(\"other {{}}\", other.is_some());
    let order: Option<Order> = natives::cast(Ticket {{ number: 3 }});
    println!(\"order {{}}\", order.is_some());
}}
"
        ),
        "same 1\nliteral false\nother false\norder false\n",
    );
}

/// Review 2: a closure native code feeds a cast value gets its own type,
/// and one of another type isn't fed, as in Rust.
#[test]
fn a_closure_native_code_feeds_takes_only_its_own_type() {
    runs(
        &format!(
            "{TYPES}
fn main() {{
    let number = natives::apply(Ticket {{ number: 3 }}, |ticket: Ticket| ticket.number);
    println!(\"applied {{number}}\");
    let length = natives::apply(Envelope {{ value: 3_i64 }}, |envelope: Envelope<String>| {{
        envelope.value.len() as i64
    }});
    println!(\"length {{length}}\");
}}
"
        ),
        "applied 3\nlength -1\n",
    );
}

/// Review 3: `T: Clone` is checked on the script type, as rustc does.
#[test]
fn a_clone_bound_is_checked_on_the_script_type() {
    runs(
        &format!(
            "{TYPES}
fn main() {{
    let (first, second) = natives::duplicate(Envelope {{ value: 2 }});
    println!(\"duplicated {{}} {{}}\", first.value, second.value);
}}
"
        ),
        "duplicated 2 2\n",
    );
    refused(
        &format!(
            "{TYPES}
fn main() {{
    let (first, second) = natives::duplicate(Ticket {{ number: 2 }});
    println!(\"wrong {{}} {{}}\", first.number, second.number);
}}
"
        ),
        "Clone",
    );
}

/// Review 8: native `Clone` and `Debug` are the script type's own.
#[test]
fn native_clone_and_debug_are_the_script_types_own() {
    runs(
        &format!(
            "{TYPES}
struct Counter {{
    count: i64,
}}

impl Clone for Counter {{
    fn clone(&self) -> Self {{
        Counter {{ count: self.count + 1 }}
    }}
}}

fn main() {{
    let (copy, original) = natives::duplicate(Counter {{ count: 1 }});
    println!(\"cloned {{}} {{}}\", copy.count, original.count);
    println!(\"{{}}\", natives::show(Envelope {{ value: Some(2) }}));
}}
"
        ),
        "cloned 2 1\nEnvelope { value: Some(2) }\n",
    );
}

/// Review 9: a script value holding a native one passes through whole.
#[test]
fn a_script_value_holding_a_native_one_passes_through_whole() {
    runs(
        "
struct Holder {
    resource: natives::Resource,
}

fn main() {
    let holder = natives::identity(Holder { resource: natives::Resource::new(3) });
    println!(\"held {}\", holder.resource.id());
}
",
        "held 3\n",
    );
}

/// `-> impl Labelled` of the native crate's trait is rustc's opaque type:
/// the body's type, here `natives::Resource`, also through another such
/// function, a path from the crate's root and an `Option`.
#[test]
fn an_opaque_output_of_a_native_trait_is_its_bodys_type() {
    runs(
        "
use natives::Labelled;

fn make(id: i64) -> impl Labelled {
    natives::Resource::new(id)
}

fn wrapped() -> impl natives::Labelled {
    make(3)
}

fn maybe(id: i64) -> Option<impl Labelled> {
    if id > 0 { Some(make(id)) } else { None }
}

fn main() {
    println!(\"{}\", make(7).label());
    println!(\"{}\", wrapped().label());
    println!(\"{}\", maybe(2).map(|made| made.label()).unwrap_or_default());
}
",
        "resource 7\nresource 3\nresource 2\n",
    );
    // A function whose hidden type is its own has none, as rustc says.
    refused(
        "
use natives::Labelled;

fn endless() -> impl Labelled {
    endless()
}

fn main() {
    println!(\"wrong {}\", endless().label());
}
",
        "impl natives::Labelled",
    );
}

/// `R: SampleRange<T>` and its `impl<T> SampleRange<T> for Range<T>`
/// give `T` the range's floats, as rustc infers it: an `f32` from `f32`
/// bounds and from literals an `f32` binding takes.
#[test]
fn a_range_gives_a_generic_bound_its_items_type() {
    runs(
        "
fn main() {
    let width: f32 = 8.0;
    let left = natives::pick((-width / 2.0)..(width / 2.0));
    let size: f32 = natives::pick(0.5..1.0);
    let wide = natives::pick(2.0_f64..3.0);
    println!(\"{left} {size} {wide}\");
}
",
        "-4 0.5 2\n",
    );
    // A range of integers or chars is that `Range<T>` too, not an iterator
    // over it: `T` is `usize` from a `len()`, `u8` from a typed bound and
    // `char` from chars.
    runs(
        "
fn main() {
    let items = vec![1, 2, 3];
    let index = natives::pick(1..items.len());
    let small = natives::pick(7u8..9);
    let letter = natives::pick('!'..'z');
    println!(\"{} {small} {}\", items[index], letter.to_string());
}
",
        "2 7 !\n",
    );
}

/// A marked `main` runs on the process's main thread, where winit's
/// event loop must run, and a native panic it meets is reported with
/// Rust's status instead of aborting (D551.6 d, e).
#[test]
fn a_marked_main_runs_on_the_main_thread_and_reports_a_native_panic() {
    runs(
        "
fn main() {
    println!(\"main thread {}\", natives::on_main_thread());
}
",
        "main thread true\n",
    );
    let (_, success, stdout, stderr) = run("
fn main() {
    let text = String::from(\"boom\");
    let size = text.len() as i64;
    println!(\"wrong {}\", natives::fail(text) + size);
}
");
    assert!(
        !success
            && !stdout.contains("wrong")
            && stderr.contains("boom")
            && !stderr.contains("ArgBackup"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// Freya's builder shapes (D551.6). A glob import brings `Lazy<B>`,
/// whose `flag(self, bool) -> Self` names its callable parameter `B`:
/// that item is refused, and the rest of the crate installs. A trait's
/// default method returning `Self` is a method of each type the trait
/// is implemented for, through a blanket implementation (`expanded`),
/// an empty one (`center`), or with a generic parameter (`child`).
#[test]
fn a_default_method_returning_self_builds_through_each_impl() {
    runs(
        "
use natives::*;

fn main() {
    let made = rect().expanded().center().child(\"Hello\").child(String::from(\"World\"));
    println!(\"{}\", made.describe());
}
",
        "rect 100 true [Hello, World]\n",
    );
}

/// Freya's `WindowConfig::new(impl Into<AppComponent>)`, which takes any
/// `F: Fn() -> E + 'static` through `From<F> for AppComponent`: a named
/// function of no inputs or a closure. Native code calls it back from
/// inside a native call, which lends it the arena, and it gives back a
/// native value (D551.6 c).
#[test]
fn a_function_or_closure_converts_into_a_native_callable() {
    runs(
        "
use natives::*;

fn app() -> impl IntoElement {
    rect().expanded().center().child(\"Hello\")
}

fn main() {
    println!(\"{}\", WindowConfig::new(app).launch(2));
    let label = String::from(\"closure\");
    println!(\"{}\", WindowConfig::new(move || rect().child(label.clone())).launch(1));
}
",
        "rect 100 true [Hello]; rect 100 true [Hello]\nrect 0 false [closure]\n",
    );
}

/// Freya's `on_press(impl Into<EventHandler<Event<PressEventData>>>)`, a
/// default method with `impl<H: FnMut(D) + 'static, D> From<H> for
/// EventHandler<D>`: a closure that takes a native event, and one that
/// changes a native count, fire when native code presses the element.
#[test]
fn native_presses_fire_script_closures_and_the_runners_own_tests_pass() {
    let root = runs(
        "
use natives::*;

fn app() -> impl IntoElement {
    rect().child(\"Hi\").on_press(|_| println!(\"pressed\"))
}

fn main() {
    println!(\"{}\", WindowConfig::new(app).click(2));
    let count = Count::new();
    let shown = count.clone();
    let window = WindowConfig::new(move || {
        let pressed = shown.clone();
        rect()
            .child(format!(\"count {}\", shown.get()))
            .on_press(move |_| pressed.add(1))
    });
    println!(\"{}\", window.click(3));
    println!(\"final {}\", count.get());
}
",
        "pressed\npressed\nrect 0 false [Hi]; rect 0 false [Hi]\nrect 0 false [count 0]; rect 0 false [count 1]; rect 0 false [count 2]\nfinal 3\n",
    );
    // The runner's runtime tests, which only a generated runner compiles:
    // a handle the protocol gave out stays alive (review 5), and a lent
    // value given back on a thread without the arena, or while a native
    // call holds it, returns to its slot (review 6, 7).
    let test = Command::new("cargo")
        .current_dir(&root)
        .args([
            "test",
            "-q",
            "--manifest-path",
            "target/rawr/runner/Cargo.toml",
        ])
        .args(["--", "native_arena_tests"])
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&test.stdout);
    assert!(
        test.status.success() && stdout.contains("5 passed"),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );
}

/// A method whose Rust output is a borrow guard: std's `Ref` and `RefMut`,
/// the crate's own `DerefMut` guard, and a projection through a storage
/// trait's generic associated types, as Freya's `State::read` and
/// `State::write` give. `*guard` reads a copy of the target and `*guard
/// op= v` stores the result back, each inside a call that drops the guard
/// before it returns, so the borrow the guard holds never reaches Rawr.
/// `op=` calls the guard's method once, after its right operand, as rustc
/// does (D554.7(f)): the tally's `+= tally.guards()` adds the two guards
/// made before it. A script `AddAssign` runs inside the guard, and its own
/// native calls reach the arena the update lends.
#[test]
fn a_method_that_gives_a_borrow_guard_is_read_and_written_through_it() {
    let (_, success, stdout, stderr) = run("
use natives::*;

struct Point {
    x: i64,
}

impl std::ops::AddAssign for Point {
    fn add_assign(&mut self, other: Point) {
        let meter = Meter::new(other.x);
        self.x += *meter.borrow();
    }
}

fn main() {
    let meter = Meter::new(1);
    *meter.borrow_mut() += 2;
    println!(\"meter {}\", *meter.borrow());

    let mut tally = Tally::new();
    *tally.lock() = 10;
    *tally.lock() += 5;
    *tally.lock() += tally.guards();
    let total = *tally.lock();
    println!(\"tally {} after {} guards\", total, tally.guards());

    let mut count = Signal::new(0_i64);
    *count.write() += 1;
    *count.write() *= 7;
    println!(\"count {}\", count.read());
    {
        let mut held = count.write();
        *held -= 2;
    }
    let shown = count.clone();
    let mut bump = move || *count.write() += 10;
    bump();
    bump();
    println!(\"bumped {}\", *shown.read());

    let mut text = Signal::new(String::from(\"a\"));
    text.write().push_str(\"b\");
    *text.write() += \"c\";
    println!(\"text {} {}\", *text.read(), text.read().len());

    let mut point = Signal::new(Point { x: 1 });
    point.write().x += 2;
    *point.write() += Point { x: 4 };
    println!(\"point {}\", point.read().x);
}
");
    assert!(success, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout, "meter 3\ntally 17 after 4 guards\ncount 7\nbumped 25\ntext abc 3\npoint 7\n",
        "stderr:\n{stderr}"
    );
}

/// Freya's crate-doc builder chain: methods that take `impl Into<T>` of a
/// native `T`, as default methods through a blanket implementation
/// (`width`, `height`) or an empty one (`padding`, and `background` on a
/// trait whose supertrait gives the storage), and as an inherent method
/// (`color`). The argument is a native `T`, an associated constant, a
/// tuple whose literals take their types from `T`'s one `From` of that
/// shape, or a float or integer `T` converts from.
#[test]
fn a_method_taking_impl_into_a_native_type_takes_what_converts_into_it() {
    let (_, success, stdout, stderr) = run("
use natives::*;

fn main() {
    let made = rect()
        .width(Size::fill())
        .height(Size::px(20.0))
        .background((35, 35, 35))
        .color(Color::WHITE)
        .padding(Gaps::new_all(12.))
        .child(\"Hi\");
    println!(\"{}\", made.styled());
    let other = rect()
        .padding(4.0)
        .background(Color::BLACK)
        .color((1, 2, 3, 0.5));
    println!(\"{}\", other.styled());
}
");
    assert!(success, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout,
        "Fill Pixels(20.0) Gaps { top: 12.0, right: 12.0 } Color(Color(4280492835)) Color(Color(4294967295))\n\
         Inner Inner Gaps { top: 4.0, right: 4.0 } Color(Color(4278190080)) Color(Color(2130772483))\n",
        "stderr:\n{stderr}"
    );
}

/// A program's types and modules may take any name, those of the runner's
/// own items among them (#1163): each type crosses as its own Rust type
/// (D594), which the runner declares where the program names it, and the
/// runner's own items are where no program can name them.
#[test]
fn a_program_type_may_take_the_name_of_a_runner_item() {
    runs(
        "
#[derive(Debug, Clone)]
pub struct Program {
    engine: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Tick(i64),
}

#[derive(Debug, Clone)]
struct Error(String);

#[derive(Debug, Clone)]
struct Engine;

#[derive(Debug, Clone)]
struct Runner;

#[derive(Debug, Clone)]
struct State;

#[derive(Debug, Clone)]
struct Application;

#[derive(Debug, Clone)]
struct Arena;

#[derive(Debug, Clone)]
struct NativeHandle;

#[derive(Debug, Clone)]
struct Started;

#[derive(Debug, Clone)]
struct Clone;

mod rawr_surface {
    #[derive(Debug, Clone)]
    pub struct Adapter {
        pub arguments: i64,
    }
}

fn main() {
    let program = natives::identity(Program { engine: 7 });
    println!(\"{}\", program.engine);
    println!(\"{}\", natives::show(Message::Tick(3)));
    let (a, b) = natives::duplicate(Error(\"no\".to_string()));
    println!(\"{a:?} {b:?}\");
    println!(
        \"{} {} {} {}\",
        natives::show(Engine),
        natives::show(Runner),
        natives::show(State),
        natives::show(Application),
    );
    println!(
        \"{} {} {} {}\",
        natives::show(Arena),
        natives::show(NativeHandle),
        natives::show(Started),
        natives::show(Clone),
    );
    println!(\"{}\", natives::show(rawr_surface::Adapter { arguments: 2 }));
}
",
        "7\nTick(3)\nError(\"no\") Error(\"no\")\nEngine Runner State Application\n\
         Arena NativeHandle Started Clone\nAdapter { arguments: 2 }\n",
    );
}
