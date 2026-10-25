//! State carried from one runner's program to the next one's.

use rawr::{application, host, Engine, Shared};

use super::Adapter;

fn source(view: &str, extra_field: &str) -> String {
    format!(
        r#"
use iced::widget::{{button, column, text}};
use iced::{{Color, Element}};
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {{ Idle, Busy(i64) }}
struct State {{ count: i64, label: String, mode: Mode, last: Option<char>, {extra_field} }}
#[derive(Debug, Clone)]
enum Message {{ Increment }}
impl State {{
    fn new() -> State {{
        State {{ count: 0, label: "start".to_string(), mode: Mode::Idle, last: None, {extra_value} }}
    }}
    fn update(&mut self, message: Message) {{
        match message {{
            Message::Increment => {{
                self.count += 1;
                self.mode = Mode::Busy(self.count);
                self.last = Some('x');
                self.label = format!("n{{}}", self.count);
            }}
        }}
    }}
    fn view(&self) -> Element<'_, Message> {{
        column![
            button(text("Increment")).on_press(Message::Increment),
            text(format!("{view} {{}} {{}} {{:?}} {{:?}}", self.count, self.label, self.mode, self.last)),
        ]
    }}
}}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view).run()
}}
"#,
        extra_value = match extra_field.split_once(':') {
            Some((name, ty)) if ty.contains("Color") => format!("{name}: Color::BLACK"),
            Some((name, _)) => format!("{name}: 0"),
            None => String::new(),
        },
    )
}

fn boot(source: &str) -> Adapter {
    let mut engine = Engine::new();
    crate::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    crate::install_test_runtime(&mut registry);
    let launch: application::BoundLaunch = launch.bind(registry).unwrap();
    Adapter::boot(launch).unwrap()
}

fn increment(adapter: &mut Adapter) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click("Increment").unwrap();
    let message = simulator.into_messages().next().unwrap();
    adapter.update(message).unwrap();
}

fn shows(adapter: &Adapter, line: &str) -> bool {
    iced_test::simulator(adapter.view()).find(line).is_ok()
}

fn export(adapter: &Adapter) -> Option<Vec<u8>> {
    adapter.export_state_authority(adapter.launch.static_authority().unwrap())
}

fn seed(adapter: &mut Adapter, bytes: &[u8]) -> bool {
    let launch = adapter.launch.static_authority().unwrap().clone();
    adapter.seed_state_authority(&launch, bytes, None)
}

#[test]
fn plain_state_carries_into_the_next_program() {
    let mut old = boot(&source("old", ""));
    increment(&mut old);
    increment(&mut old);
    let bytes = export(&old).expect("plain script data is carried");

    let mut new = boot(&source("new", ""));
    assert!(shows(&new, "new 0 start Idle None"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, "new 2 n2 Busy(2) Some('x')"));
    increment(&mut new);
    assert!(shows(&new, "new 3 n3 Busy(3) Some('x')"));
}

#[test]
fn a_changed_state_type_boots_fresh() {
    let mut old = boot(&source("old", ""));
    increment(&mut old);
    let bytes = export(&old).unwrap();

    let mut new = boot(&source("new", "extra: i64"));
    assert!(!seed(&mut new, &bytes));
    assert!(shows(&new, "new 0 start Idle None"));
}

#[test]
fn colors_and_themes_in_state_are_carried() {
    let mut old = boot(&source("old", "tint: Color"));
    increment(&mut old);
    let bytes = export(&old).expect("a Color is plain data");
    let mut new = boot(&source("new", "tint: Color"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, "new 1 n1 Busy(1) Some('x')"));

    let themes = |tag: &str| {
        COLLECTIONS.replace("TAG", tag).replace(
            "boxed: Box<Option<char>> }",
            "boxed: Box<Option<char>>, themes: Vec<Theme>, tint: Option<Color> }",
        )
        .replace(
            "self.pair = (n, format!(\"p{}\", n));",
            "self.pair = (n, format!(\"p{}\", n)); self.themes = vec![Theme::Dracula, Theme::custom(\"Mine\".to_string(), Seed { primary: Color::from_rgb8(1, 2, 3), ..Seed::LIGHT })]; self.tint = Some(Color::from_rgb8(9, 8, 7));",
        )
        .replace(
            "self.boxed)),",
            "self.boxed)), text(format!(\"{} {} {}\", self.themes.len(), self.themes.get(1).map(|t| t.palette().primary.base.color) == Some(Color::from_rgb8(1, 2, 3)), self.tint == Some(Color::from_rgb8(9, 8, 7)))),",
        )
        .replace("use iced::Element;", "use iced::{Color, Element, Theme};\nuse iced::theme::palette::Seed;")
    };
    let mut old = boot(&themes("old"));
    increment(&mut old);
    let bytes = export(&old).expect("Themes are plain data");
    let mut new = boot(&themes("new"));
    assert!(shows(&new, "0 false false"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, "2 true true"));
}

#[test]
fn malformed_bytes_boot_fresh() {
    let mut old = boot(&source("old", ""));
    increment(&mut old);
    let mut bytes = export(&old).unwrap();
    bytes.truncate(bytes.len() - 1);

    let mut new = boot(&source("new", ""));
    assert!(!seed(&mut new, &bytes));
    assert!(!seed(&mut new, &[]));
    assert!(shows(&new, "new 0 start Idle None"));
}

const COLLECTIONS: &str = r#"
use std::collections::{BTreeMap, HashSet};
use iced::widget::{button, column, text};
use iced::Element;
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode { Idle, Busy }
#[derive(Default)]
struct State { items: Vec<i64>, names: BTreeMap<String, Mode>, seen: HashSet<i64>, pair: (i64, String), boxed: Box<Option<char>> }
#[derive(Debug, Clone)]
enum Message { Increment }
impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                let n = self.items.len() as i64 + 1;
                self.items.push(n);
                self.names.insert(format!("k{}", n), Mode::Busy);
                self.seen.insert(n % 2);
                self.pair = (n, format!("p{}", n));
            }
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            button(text("Increment")).on_press(Message::Increment),
            text(format!("TAG {:?} {:?} {:?} {:?} {:?}", self.items, self.names, self.seen, self.pair, self.boxed)),
        ]
    }
}
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
"#;

#[test]
fn collections_and_tuples_in_state_boot_and_carry() {
    let mut old = boot(&COLLECTIONS.replace("TAG", "old"));
    assert!(shows(&old, "old [] {} {} (0, \"\") None"));
    increment(&mut old);
    increment(&mut old);
    let shown = "[1, 2] {\"k1\": Busy, \"k2\": Busy} {0, 1} (2, \"p2\") None";
    assert!(shows(&old, &format!("old {shown}")));
    let bytes = export(&old).expect("collections are plain data");

    let mut new = boot(&COLLECTIONS.replace("TAG", "new"));
    assert!(seed(&mut new, &bytes));
    assert!(shows(&new, &format!("new {shown}")));
}

/// State kept by one runner across script edits. Each edit binds its
/// program to the next generation of the runner's one registry.
mod in_one_runner {
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use rawr::host::{self, facade, item, signature, type_ref};
    use rawr::reload::scheduler::{publication, Admission, Gate};
    use rawr::{Dynamic, EvalAltResult, Position};

    /// A Rust value the way the bridge holds an imported one: its witness
    /// attests in any generation of the runner that made it.
    #[derive(Clone)]
    struct Gauge {
        witness: host::runtime::Witness,
        level: i64,
    }

    fn gauge_facade() -> host::facade::Definition {
        let mut builder = facade::Builder::new("gauge", facade::Abi::new(1).unwrap()).unwrap();
        builder.type_::<Gauge>("Gauge", &["T"], &[]).unwrap();
        let gauge =
            || type_ref::Type::applied("Gauge", [type_ref::Type::parameter("T").unwrap()]).unwrap();
        let mut new = signature::Builder::new();
        new.type_parameter("T", &[]).unwrap();
        new.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        new.output(gauge());
        builder
            .item(item::Definition::associated_function("Gauge::new", new.finish()).unwrap())
            .unwrap();
        let mut level = signature::Builder::new();
        level.type_parameter("T", &[]).unwrap();
        level.output(type_ref::Type::builtin(type_ref::Builtin::I64));
        builder
            .item(
                item::Definition::method(
                    "Gauge::level",
                    rawr::receiver::Mode::Shared,
                    level.finish(),
                )
                .unwrap(),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn gauge_runtime() -> host::runtime::Facade {
        let mut builder = host::runtime::Builder::new(gauge_facade());
        builder
            .bind_with_context("gauge::Gauge::new", |call, args| {
                let level = args[0]
                    .as_int()
                    .map_err(|_| error("Gauge::new takes an i64"))?;
                let witness = call
                    .output_witness()
                    .map_err(|_| error("Gauge::new witness"))?;
                Ok(Dynamic::from(Gauge { witness, level }))
            })
            .unwrap();
        builder
            .bind("gauge::Gauge::level", |args| {
                let gauge = args[0]
                    .read_lock::<Gauge>()
                    .ok_or_else(|| error("not a Gauge"))?;
                Ok(Dynamic::from(gauge.level))
            })
            .unwrap();
        builder
            .bind_type_attestation("gauge::Gauge", |value, expected| {
                let gauge = value.read_lock::<Gauge>()?;
                expected.attest_native_witness(Some(&gauge.witness))
            })
            .unwrap();
        builder.finish()
    }

    fn error(message: &str) -> Box<EvalAltResult> {
        EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
    }

    const GAUGE: &str = r#"
use iced::widget::{button, column, text};
use iced::Element;
struct State { gauge: gauge::Gauge<i64> }
#[derive(Debug, Clone)]
enum Message { Bump }
impl State {
    fn new() -> State { State { gauge: gauge::Gauge::new(0) } }
    fn update(&mut self, message: Message) {
        match message {
            Message::Bump => self.gauge = gauge::Gauge::new(self.gauge.level() + 1),
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            button(text("Bump")).on_press(Message::Bump),
            text(format!("TAG {}", self.gauge.level())),
        ]
    }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;

    const COMBO: &str = r#"
use iced::widget::{column, combo_box, text};
use iced::Element;
struct State { names: combo_box::State<String>, chosen: Option<String> }
#[derive(Debug, Clone)]
enum Message { Chosen(String) }
impl State {
    fn new() -> State {
        State { names: combo_box::State::new(vec!["a".to_string(), "b".to_string()]), chosen: None }
    }
    fn update(&mut self, message: Message) {
        match message {
            Message::Chosen(name) => self.chosen = Some(name),
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![
            text("TAG"),
            combo_box(&self.names, "Pick", self.chosen.as_ref(), Message::Chosen),
        ]
    }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#;

    /// `main` writes statements before its app, as iced's examples set up
    /// tracing. `PRELUDE` is those statements.
    const PRELUDED: &str = r#"
use iced::widget::{button, column, text};
use iced::Element;
#[derive(Default)]
struct State { count: i64 }
#[derive(Debug, Clone)]
enum Message { Bump }
impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Bump => self.count += 1,
        }
    }
    fn view(&self) -> Element<'_, Message> {
        column![button(text("Bump")).on_press(Message::Bump), text(format!("TAG {}", self.count))]
    }
}
pub fn main() -> iced::Result {
    PRELUDE
    iced::application(State::default, State::update, State::view).run()
}
"#;

    struct Runner {
        root: std::path::PathBuf,
        engine: rawr::Shared<rawr::Engine>,
        target: rawr::project::Target,
        registry: host::runtime::Registry,
        publication: Arc<publication::Slot>,
        host: crate::program::Host,
    }

    impl Runner {
        fn start(name: &str, source: &str) -> Self {
            Self::try_start(name, source).unwrap()
        }

        fn try_start(name: &str, source: &str) -> Result<Self, crate::program::install::Error> {
            let root = std::env::temp_dir().join(format!(
                "rawr-carry-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            // Each runner is its own package, as `main` runs once per
            // package in a process.
            fs::write(
                root.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"carry-{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
                ),
            )
            .unwrap();
            crate::write_marked(&root, "src/main.rs", source.replace("TAG", "old"));
            let mut engine = rawr::Engine::new();
            crate::install(&mut engine).unwrap();
            engine.register_host_facade(gauge_facade()).unwrap();
            let engine = rawr::Shared::new(engine);
            let mut registry = host::runtime::Registry::new();
            crate::install_test_runtime(&mut registry);
            registry.install(gauge_runtime()).unwrap();
            let target = rawr::project::Target::discover(&root).unwrap();
            let first = engine.compile_package_generation(&target).unwrap();
            let publication = Arc::new(publication::Slot::new(
                rawr::reload::source::Inputs::from_graph(first.source()),
            ));
            let diagnostic = first.source().entry_source().diagnostic().clone();
            let bootstrap = first.bind(&engine, registry.next_generation()).unwrap();
            let host = crate::program::Host::bootstrap(bootstrap, Some(diagnostic));
            let host = match host {
                Ok(host) => host,
                Err(error) => {
                    fs::remove_dir_all(&root).ok();
                    return Err(error);
                }
            };
            Ok(Self {
                root,
                engine,
                target,
                registry,
                publication,
                host,
            })
        }

        /// Install `source` as a script edit; whether the edit applied.
        fn edit(&mut self, source: &str) -> bool {
            crate::write_marked(&self.root, "src/main.rs", source);
            let next = self
                .engine
                .compile_package_generation(&self.target)
                .unwrap();
            let now = Instant::now();
            let gate = Gate::new(Duration::ZERO);
            gate.observe(now).unwrap();
            let snapshot = rawr::reload::source::Snapshot::from_graph(next.source());
            let candidate = gate
                .start(now)
                .unwrap()
                .complete((next, snapshot))
                .stage_inputs(&self.publication);
            match self.host.install(
                &gate,
                &self.publication,
                candidate,
                &self.engine,
                self.registry.next_generation(),
            ) {
                Ok(Admission::Current(outcome)) => {
                    let _task = outcome.into_task();
                    true
                }
                Ok(Admission::Obsolete(())) => panic!("a fresh edit must stay current"),
                Err(_) => false,
            }
        }

        /// Install `source` as a script edit; whether it cancelled a task.
        fn edit_cancelling(&mut self, source: &str) -> bool {
            crate::write_marked(&self.root, "src/main.rs", source);
            let next = self
                .engine
                .compile_package_generation(&self.target)
                .unwrap();
            let now = Instant::now();
            let gate = Gate::new(Duration::ZERO);
            gate.observe(now).unwrap();
            let snapshot = rawr::reload::source::Snapshot::from_graph(next.source());
            let candidate = gate
                .start(now)
                .unwrap()
                .complete((next, snapshot))
                .stage_inputs(&self.publication);
            match self.host.install(
                &gate,
                &self.publication,
                candidate,
                &self.engine,
                self.registry.next_generation(),
            ) {
                Ok(Admission::Current(outcome)) => {
                    let cancelled = outcome.cancelled();
                    assert_eq!(
                        outcome.note(None).as_deref(),
                        cancelled.then_some("The reload cancelled a running task.")
                    );
                    let _task = outcome.into_task();
                    cancelled
                }
                Ok(Admission::Obsolete(())) => panic!("a fresh edit must stay current"),
                Err(error) => panic!("the edit must apply: {error:?}"),
            }
        }

        /// Click `label` and start the task its update returns, which
        /// nothing drives here, so it keeps running.
        fn click_and_start(&mut self, label: &str) {
            let mut simulator = iced_test::simulator(self.host.view());
            simulator.click(label).unwrap();
            let message = simulator.into_messages().next().unwrap();
            let transition = self.host.update(message).unwrap();
            drop(self.host.lower(transition));
        }

        fn click(&mut self, label: &str) {
            let mut simulator = iced_test::simulator(self.host.view());
            simulator.click(label).unwrap();
            let message = simulator.into_messages().next().unwrap();
            drop(self.host.update(message).unwrap());
        }

        fn shows(&self, line: &str) -> bool {
            iced_test::simulator(self.host.view()).find(line).is_ok()
        }
    }

    impl Drop for Runner {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }

    #[test]
    fn a_script_edit_keeps_an_imported_value_and_its_state() {
        let mut runner = Runner::start("native", GAUGE);
        runner.click("Bump");
        runner.click("Bump");
        assert!(runner.shows("old 2"));

        assert!(runner.edit(&GAUGE.replace("TAG", "new")));
        assert!(runner.shows("new 2"));
        runner.click("Bump");
        assert!(runner.shows("new 3"));
        assert!(runner.edit(&GAUGE.replace("TAG", "newer")));
        assert!(runner.shows("newer 3"));
    }

    #[test]
    fn a_combo_box_state_from_an_earlier_generation_is_still_refused() {
        let mut runner = Runner::start("combo", COMBO);
        assert!(runner.shows("old"));
        assert!(!runner.edit(&COMBO.replace("TAG", "new")));
        assert!(runner.shows("old"));
    }

    #[test]
    fn mains_statements_run_before_the_first_boot() {
        let source = PRELUDED.replace("PRELUDE", r#"panic!("main ran");"#);
        let error = Runner::try_start("prelude-runs", &source).err().unwrap();
        assert!(format!("{error:?}").contains("main ran"), "{error:?}");
    }

    #[test]
    fn an_edit_to_mains_statements_waits_for_a_restart() {
        let first = PRELUDED.replace("PRELUDE", "let ready = true;");
        let mut runner = Runner::start("prelude-once", &first);
        runner.click("Bump");
        assert!(runner.shows("old 1"));
        assert_eq!(runner.host.prelude_note(), None);

        // The edit applies and keeps the State. The new statements do not
        // run, here or on a reset.
        let edited = PRELUDED
            .replace("PRELUDE", r#"panic!("main ran again");"#)
            .replace("TAG", "new");
        assert!(runner.edit(&edited));
        assert!(runner.shows("new 1"));
        assert_eq!(
            runner.host.prelude_note().as_deref(),
            Some("This edit to `main` takes effect when `rawr run` restarts.")
        );
        drop(runner.host.reset().unwrap());
        assert!(runner.shows("new 0"));
    }

    /// A task started before an edit and still running. The edit cancels
    /// it, as retiring its owner does.
    const RUNNING: &str = r#"
use iced::task;
use iced::widget::{button, column, text};
use iced::{Element, Task};
#[derive(Default)]
struct State { handle: Option<task::Handle> }
#[derive(Debug, Clone)]
enum Message { Start, Done }
impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Start => {
                let (task, handle) = Task::done(Message::Done).abortable();
                self.handle = Some(handle);
                task
            }
            Message::Done => Task::none(),
        }
    }
    fn view(&self) -> Element<'_, Message> {
        let aborted = match &self.handle {
            Some(handle) => handle.is_aborted().to_string(),
            None => "none".to_string(),
        };
        column![button(text("Start")).on_press(Message::Start), text(format!("TAG {aborted}"))]
    }
}
pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view).run()
}
"#;

    #[test]
    fn a_reload_cancels_a_running_task_and_says_so() {
        let mut runner = Runner::start("cancel", RUNNING);
        assert!(!runner.edit_cancelling(&RUNNING.replace("TAG", "idle")));
        runner.click_and_start("Start");
        assert!(runner.shows("idle false"));
        assert!(runner.edit_cancelling(&RUNNING.replace("TAG", "new")));
        assert!(runner.shows("new true"));
        assert!(!runner.edit_cancelling(&RUNNING.replace("TAG", "newer")));
    }

    #[test]
    fn a_rust_change_reseeds_and_names_the_imported_type() {
        let mut runner = Runner::start("reseed", GAUGE);
        assert!(!runner.host.seed_state(&[]));
        assert_eq!(
            runner.host.take_reseeded().as_deref(),
            Some("State reseeded · App: Rust code changed · gauge::Gauge does not carry over")
        );
        assert!(runner.shows("old 0"));
    }
}
