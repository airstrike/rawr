use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rawr::reload::compatibility::Action;
use rawr::{host, project, Engine, Shared};
use rawr_iced::program::{Delivery, Host};

static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn body_only_replacement_retains_state_and_uses_new_callbacks() {
    let project = Project::new("retain");
    project.write("src/main.rs", source("1", "Increment"));
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let instance = host.adapter().instance();
    let old_generation = host.adapter().generation();
    let delayed = click(&host, "Increment");

    project.write("src/main.rs", source("2", "Add two"));
    let outcome = install(&mut host, compile(&engine, &project), &engine, registry()).unwrap();

    assert_eq!(outcome.evidence()[0].action(), Action::Retain);
    assert_eq!(host.adapter().instance(), instance);
    assert_ne!(host.adapter().generation(), old_generation);
    assert_eq!(host.generation().id(), host.adapter().generation());
    assert_eq!(host.update(delayed).unwrap().delivery(), Delivery::Dropped);
    iced_test::simulator(host.view()).find("Add two").unwrap();

    let current_message = click(&host, "Add two");
    assert_eq!(
        host.update(current_message).unwrap().delivery(),
        Delivery::Applied
    );
    iced_test::simulator(host.view()).find("2").unwrap();
}

#[test]
fn palette_only_custom_theme_rebinds_on_retained_reload() {
    let project = Project::new("palette-theme");
    project.write(
        "src/main.rs",
        palette_theme_source("First palette", "#4455aa"),
    );
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let instance = host.adapter().instance();

    assert_eq!(host.adapter().theme().native().to_string(), "First palette");

    project.write(
        "src/main.rs",
        palette_theme_source("Second palette", "#aa5544"),
    );
    let outcome = install(&mut host, compile(&engine, &project), &engine, registry()).unwrap();

    assert_eq!(outcome.evidence()[0].action(), Action::Retain);
    assert_eq!(host.adapter().instance(), instance);
    assert_eq!(
        host.adapter().theme().native().to_string(),
        "Second palette"
    );
    iced_test::simulator(host.view())
        .find("Palette theme")
        .unwrap();
}

#[test]
fn title_rebinds_on_retained_state_and_throwing_candidates_preserve_last_good_shell() {
    let project = Project::new("title-shell");
    project.write("src/main.rs", title_source("self.label", true));
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let instance = host.adapter().instance();

    assert_eq!(host.adapter().title(), "Initial");
    assert!(host.adapter().centered());
    let rename = click(&host, "Rename");
    assert_eq!(host.update(rename).unwrap().delivery(), Delivery::Applied);
    assert_eq!(host.adapter().title(), "Updated");

    project.write("src/main.rs", title_source("\"Rebound\"", true));
    let outcome = install(&mut host, compile(&engine, &project), &engine, registry()).unwrap();
    assert_eq!(outcome.evidence()[0].action(), Action::Retain);
    assert_eq!(host.adapter().instance(), instance);
    assert_eq!(host.adapter().title(), "Rebound");
    assert_eq!(host.adapter().last_good_title(), Some("Rebound"));

    let retained_generation = host.generation().clone();
    let adapter_generation = host.adapter().generation();
    let retained_failure = title_source("throw \"replacement title failed\"", true);
    let reset_failure = retained_failure
        .replace(
            "struct State { label: String }",
            "struct State { label: String, reset: bool }",
        )
        .replace(
            "State { label: \"Initial\" }",
            "State { label: \"Initial\", reset: false }",
        );
    for candidate in [retained_failure, reset_failure] {
        project.write("src/main.rs", candidate.clone());
        let Err(error) = install(&mut host, compile(&engine, &project), &engine, registry()) else {
            panic!("throwing title candidate must not replace the current owner");
        };
        let rawr_iced::program::install::Error::Diagnostic(report) = *error else {
            panic!("throwing title candidate must retain its exact diagnostic");
        };
        let label = &report.labels()[0];
        let (line, declaration) = candidate
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("fn title"))
            .expect("the candidate must retain its title declaration");
        let column = declaration
            .find("throw")
            .expect("the failing title must retain its throw expression");
        let expected = rawr::Span::new(
            rawr::Position::new((line + 2) as u16, (column + 1) as u16),
            rawr::Position::new((line + 2) as u16, (column + 7) as u16),
        );

        assert_eq!(report.code(), rawr::diagnostic::code::RUNTIME_THROW);
        assert_eq!(report.category(), rawr::diagnostic::Category::Runtime);
        assert!(report.message().contains("replacement title failed"));
        assert_eq!(report.labels().len(), 1);
        assert!(label.source().identity().ends_with("src/main.rs"));
        assert_eq!(label.source().text(), format!("// rawr\n{candidate}"));
        assert_eq!(label.span(), expected);
        assert!(Arc::ptr_eq(&host.generation(), &retained_generation));
        assert_eq!(host.adapter().generation(), adapter_generation);
        assert_eq!(host.adapter().instance(), instance);
        assert_eq!(host.adapter().title(), "Rebound");
        assert_eq!(host.adapter().last_good_title(), Some("Rebound"));
        iced_test::simulator(host.view()).find("Updated").unwrap();
    }

    project.write("src/main.rs", title_source("\"Off center\"", false));
    let outcome = install(&mut host, compile(&engine, &project), &engine, registry()).unwrap();
    assert_eq!(outcome.evidence()[0].action(), Action::Reset);
    assert_ne!(host.adapter().instance(), instance);
    assert_eq!(host.adapter().title(), "Off center");
    assert!(!host.adapter().centered());
}

#[test]
fn schema_replacement_boots_visible_fresh_state_and_rotates_instance() {
    let project = Project::new("reset");
    project.write("src/main.rs", typed_source("i64", "0"));
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let instance = host.adapter().instance();

    project.write("src/main.rs", typed_source("bool", "false"));
    let outcome = install(&mut host, compile(&engine, &project), &engine, registry()).unwrap();

    assert_eq!(outcome.evidence()[0].action(), Action::Reset);
    assert_ne!(host.adapter().instance(), instance);
    assert_eq!(host.generation().id(), host.adapter().generation());
    iced_test::simulator(host.view()).find("false").unwrap();
}

#[test]
fn view_and_boot_failure_preserve_the_exact_last_good_owner() {
    let project = Project::new("failure");
    project.write("src/main.rs", source("1", "Last good"));
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let retained = host.generation();
    let instance = host.adapter().instance();
    let generation = host.adapter().generation();

    let failing_view = source("2", "Broken")
        .replace(
            "let increment = button(\"Broken\").on_press(Message::Increment);\n        let value = text(self.value);\n        column![value, increment]",
            "self.broken()",
        )
        .replace(
            "fn view(&self) -> Element<Message>",
            "fn broken(&self) -> Element<Message> { throw \"replacement view failed\" }\n\n    fn view(&self) -> Element<Message>",
        );
    project.write("src/main.rs", &failing_view);
    let Err(error) = install(&mut host, compile(&engine, &project), &engine, registry()) else {
        panic!("current prepare failure must retain a canonical report");
    };
    let rawr_iced::program::install::Error::Diagnostic(report) = *error else {
        panic!("current prepare failure must remain a diagnostic");
    };
    assert!(rawr::diagnostic::render(&report).contains("replacement view failed"));
    assert!(Arc::ptr_eq(&host.generation(), &retained));
    assert_eq!(host.adapter().instance(), instance);
    assert_eq!(host.adapter().generation(), generation);
    iced_test::simulator(host.view()).find("Last good").unwrap();

    project.write("src/main.rs", explicit_source("bool", true));
    assert!(matches!(
        install(&mut host, compile(&engine, &project), &engine, registry()),
        Err(error)
            if matches!(
                error.as_ref(),
                rawr_iced::program::install::Error::Diagnostic(_)
            )
    ));
    assert!(Arc::ptr_eq(&host.generation(), &retained));
    assert_eq!(host.adapter().instance(), instance);
    assert_eq!(host.adapter().generation(), generation);
}

#[test]
fn public_control_preview_metadata_changes_with_the_atomic_host_commit() {
    let project = Project::new("control-preview-list");
    project.write("src/main.rs", control_source("First", "first"));
    let engine = compiler();
    let candidate = compile(&engine, &project);
    let publication = publication(candidate.source());
    let source = candidate.source().entry_source().diagnostic().clone();
    let bootstrap = candidate.bind(&engine, registry()).unwrap();
    let mut host = rawr_iced::control::Host::bootstrap(bootstrap, source).unwrap();
    assert_eq!(
        host.previews()
            .iter()
            .map(rawr_iced::control::Preview::name)
            .collect::<Vec<_>>(),
        vec!["First"]
    );

    project.write("src/main.rs", control_source("Second", "second"));
    let (gate, candidate) = scheduled(compile(&engine, &project), &publication);
    let admission = host
        .install(&gate, &publication, candidate, &engine, registry())
        .unwrap();
    assert!(matches!(
        admission,
        rawr::reload::scheduler::Admission::Current(_)
    ));

    assert_eq!(
        host.previews()
            .iter()
            .map(rawr_iced::control::Preview::name)
            .collect::<Vec<_>>(),
        vec!["Second"]
    );
}

#[test]
fn prepared_operation_starts_only_after_commit_and_poll() {
    let project = Project::new("deferred-operation-admitted");
    project.write("src/main.rs", typed_source("i64", "0"));
    let effects = effects_facade();
    let engine = compiler_with(effects.clone());
    let mut host = bootstrap_with(
        &engine,
        compile(&engine, &project),
        registry_with(effects.clone(), Arc::new(AtomicUsize::new(0))),
    );
    let starts = Arc::new(AtomicUsize::new(0));

    project.write("src/main.rs", operation_source());
    let outcome = install(
        &mut host,
        compile(&engine, &project),
        &engine,
        registry_with(effects, starts.clone()),
    )
    .unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    let _message = one_runtime_message(outcome.into_task());
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[test]
fn event_during_preparation_rejects_the_public_host_install_without_mutation() {
    let project = Project::new("obsolete-public-install");
    project.write("src/main.rs", source("1", "Retained"));
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let retained = host.generation();
    let instance = host.adapter().instance();

    project.write("src/main.rs", source("2", "Obsolete"));
    let publication = publication(host.generation().source());
    let (gate, candidate) = scheduled(compile(&engine, &project), &publication);
    gate.observe(std::time::Instant::now()).unwrap();
    let admission = host
        .install(&gate, &publication, candidate, &engine, registry())
        .unwrap();

    assert!(matches!(
        admission,
        rawr::reload::scheduler::Admission::Obsolete(())
    ));
    assert!(Arc::ptr_eq(&host.generation(), &retained));
    assert_eq!(host.adapter().instance(), instance);
    iced_test::simulator(host.view()).find("Retained").unwrap();
}

#[test]
fn another_gate_rejects_host_and_publication_admission_without_mutation() {
    let project = Project::new("cross-gate-public-install");
    project.write("src/main.rs", source("1", "Retained"));
    let engine = compiler();
    let mut host = bootstrap(&engine, compile(&engine, &project));
    let retained = host.generation();
    let instance = host.adapter().instance();
    let publication = publication(host.generation().source());
    let installed_paths = publication.read().current().paths().to_vec();

    project.write("src/main.rs", source("2", "Foreign"));
    let (_origin, candidate) = scheduled(compile(&engine, &project), &publication);
    assert_eq!(publication.read().staged(), 1);
    let foreign = rawr::reload::scheduler::Gate::new(std::time::Duration::ZERO);
    foreign.observe(std::time::Instant::now()).unwrap();

    let admission = host
        .install(&foreign, &publication, candidate, &engine, registry())
        .unwrap();

    assert!(matches!(
        admission,
        rawr::reload::scheduler::Admission::Obsolete(())
    ));
    assert!(Arc::ptr_eq(&host.generation(), &retained));
    assert_eq!(host.adapter().instance(), instance);
    assert_eq!(publication.read().staged(), 0);
    assert_eq!(publication.read().current().paths(), installed_paths);
    iced_test::simulator(host.view()).find("Retained").unwrap();
}

fn compiler() -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    Shared::new(engine)
}

fn compiler_with(facade: host::facade::Definition) -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine.register_host_facade(facade).unwrap();
    Shared::new(engine)
}

fn registry() -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
}

fn registry_with(
    facade: host::facade::Definition,
    starts: Arc<AtomicUsize>,
) -> host::runtime::Registry {
    let operation = facade
        .items()
        .find(|item| item.path() == "effects::accept")
        .unwrap()
        .clone();
    let mut binding = host::runtime::Builder::new(facade);
    binding
        .bind_operation(
            &operation,
            move |arguments| {
                starts.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert_eq!(arguments.len(), 2);
                    Ok::<_, ()>(rawr::Dynamic::from(7_i64))
                }
            },
            |_| rawr::Dynamic::from(-1_i64),
        )
        .unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry.install(binding.finish()).unwrap();
    registry
}

fn effects_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("effects", host::facade::Abi::new(1).unwrap()).unwrap();
    let mut accept = host::signature::Builder::new();
    accept.type_parameter("First", &[]).unwrap();
    accept.type_parameter("Second", &[]).unwrap();
    accept.parameter(host::type_ref::Type::parameter("First").unwrap());
    accept.parameter(host::type_ref::Type::parameter("Second").unwrap());
    accept.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
    builder
        .item(host::item::Definition::operation("accept", accept.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn compile(engine: &Shared<Engine>, project: &Project) -> rawr::reload::Candidate {
    let target = project::Target::discover(project.path()).unwrap();
    engine.compile_package_generation(&target).unwrap()
}

fn bootstrap(engine: &Shared<Engine>, candidate: rawr::reload::Candidate) -> Host {
    bootstrap_with(engine, candidate, registry())
}

fn bootstrap_with(
    engine: &Shared<Engine>,
    candidate: rawr::reload::Candidate,
    registry: host::runtime::Registry,
) -> Host {
    let source = candidate.source().entry_source().diagnostic().clone();
    let bootstrap = candidate.bind(engine, registry).unwrap();
    Host::bootstrap(bootstrap, Some(source)).unwrap()
}

fn install(
    host: &mut Host,
    candidate: rawr::reload::Candidate,
    engine: &Shared<Engine>,
    registry: host::runtime::Registry,
) -> Result<rawr_iced::program::install::Outcome, Box<rawr_iced::program::install::Error>> {
    let publication = publication(host.generation().source());
    let (gate, candidate) = scheduled(candidate, &publication);
    match host.install(&gate, &publication, candidate, engine, registry)? {
        rawr::reload::scheduler::Admission::Current(outcome) => Ok(outcome),
        rawr::reload::scheduler::Admission::Obsolete(()) => {
            panic!("fresh deterministic candidate must remain current")
        }
    }
}

fn publication(
    graph: &rawr::reload::source::Graph,
) -> Arc<rawr::reload::scheduler::publication::Slot> {
    Arc::new(rawr::reload::scheduler::publication::Slot::new(
        rawr::reload::source::Inputs::from_graph(graph),
    ))
}

fn scheduled(
    candidate: rawr::reload::Candidate,
    publication: &Arc<rawr::reload::scheduler::publication::Slot>,
) -> (
    rawr::reload::scheduler::Gate,
    rawr::reload::scheduler::Candidate<(
        rawr::reload::Candidate,
        rawr::reload::scheduler::publication::Staged,
    )>,
) {
    let now = std::time::Instant::now();
    let gate = rawr::reload::scheduler::Gate::new(std::time::Duration::ZERO);
    gate.observe(now).unwrap();
    let snapshot = rawr::reload::source::Snapshot::from_graph(candidate.source());
    let candidate = gate
        .start(now)
        .unwrap()
        .complete((candidate, snapshot))
        .stage_inputs(publication);
    (gate, candidate)
}

fn one_runtime_message(
    task: iced::Task<rawr_iced::runtime::Message>,
) -> rawr_iced::runtime::Message {
    use iced_test::futures::futures::StreamExt;

    let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
    iced_test::futures::futures::executor::block_on(async move {
        match stream.next().await {
            Some(iced_test::runtime::Action::Output(message)) => message,
            Some(action) => panic!("expected message output, found {action:?}"),
            None => panic!("operation task must emit one message"),
        }
    })
}

fn click(host: &Host, label: &str) -> rawr_iced::runtime::Message {
    let mut ui = iced_test::simulator(host.view());
    let _ = ui.click(label).unwrap();
    ui.into_messages()
        .next()
        .expect("button click must emit one script message")
}

fn source(amount: &str, label: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::{{button, column, text}};

#[derive(Default)]
struct Counter {{ value: i64 }}

#[derive(Debug, Clone, Copy)]
enum Message {{ Increment }}

impl Counter {{
    fn update(&mut self, message: Message) {{
        match message {{ Message::Increment => self.value += {amount}, }}
    }}

    fn view(&self) -> Element<Message> {{
        let increment = button("{label}").on_press(Message::Increment);
        let value = text(self.value);
        column![value, increment]
    }}
}}

pub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}
"#,
    )
}

fn palette_theme_source(name: &str, primary: &str) -> String {
    format!(
        r##"
use iced::Theme;
use iced::Element;
use iced::widget::button;
use rawr_iced::theme::{{Palette, custom}};

struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}

impl State {{
    fn new() -> State {{ State {{}} }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<Message> {{
        button("Palette theme").style(button::primary)
    }}
    fn theme(&self) -> Theme {{
        let palette = Palette::new("#10131a", "#e7eaf0", "{primary}", "#78dba9", "#f4cc67", "#ff7d8b");
        custom("{name}", palette)
    }}
}}

pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view)
        .theme(State::theme)
        .run()
}}
"##,
    )
}

fn typed_source(field: &str, value: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::text;

struct State {{ value: {field} }}

#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

impl State {{
    fn boot() -> State {{ State {{ value: {value} }} }}
    fn update(&mut self, message: Message) {{ match message {{ Message::Tick => (), }} }}
    fn view(&self) -> Element<Message> {{ text(self.value) }}
}}

pub fn main() -> iced::Result {{
    iced::application(State::boot, State::update, State::view).run()
}}
"#,
    )
}

fn title_source(title: &str, centered: bool) -> String {
    let centered = if centered { ".centered()" } else { "" };
    format!(
        r#"
use iced::Element;
use iced::widget::{{button, column, text}};

struct State {{ label: String }}
#[derive(Debug, Clone, Copy)] enum Message {{ Rename }}

impl State {{
    fn boot() -> State {{ State {{ label: "Initial" }} }}
    fn update(&mut self, message: Message) {{
        match message {{ Message::Rename => self.label = "Updated", }}
    }}
    fn view(&self) -> Element<Message> {{
        column![text(self.label), button("Rename").on_press(Message::Rename)]
    }}
    fn title(&self) -> String {{ {title} }}
}}

pub fn main() -> iced::Result {{
    iced::application(State::boot, State::update, State::view)
        .title(State::title){centered}
        .run()
}}
"#,
    )
}

fn explicit_source(field: &str, fail: bool) -> String {
    let boot = if fail {
        "throw \"replacement boot failed\""
    } else {
        "State { value: 0 }"
    };
    format!(
        r#"
use iced::Element;
use iced::widget::text;

struct State {{ value: {field} }}
#[derive(Debug, Clone, Copy)]
enum Message {{ Tick }}

impl State {{
    fn boot() -> State {{ {boot} }}
    fn update(&mut self, message: Message) {{ match message {{ Message::Tick => (), }} }}
    fn view(&self) -> Element<Message> {{ text("explicit") }}
}}

pub fn main() -> iced::Result {{
    iced::application(State::boot, State::update, State::view).run()
}}
"#,
    )
}

fn control_source(name: &str, label: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<Message> {{ text("application") }}
}}

#[preview(name = "{name}")]
fn sample() -> Element<'_, ()> {{ text("{label}") }}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#,
    )
}

fn operation_source() -> &'static str {
    r#"
use effects::accept;
use iced::Task;
use iced::Element;
use iced::widget::text;

struct State { value: bool }
#[derive(Debug, Clone, Copy)] enum Message { Loaded(i64) }
pub fn loaded(value: i64) -> Message { Message::Loaded(value) }
impl State {
    fn boot() -> (State, Task<Message>) {
        (State { value: false }, Task::perform(accept(1, 2), loaded))
    }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text(self.value) }
}

pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rawr-reload-installation-{}-{id}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").unwrap();
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, path: &str, source: impl AsRef<str>) {
        let path = self.root.join(path);
        // A `.rs` file is a Rawr file: it starts with `// rawr`.
        let marked;
        let source = if path.extension().is_some_and(|extension| extension == "rs") {
            marked = format!("// rawr\n{}", source.as_ref());
            marked.as_str()
        } else {
            source.as_ref()
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, source).unwrap();
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}
