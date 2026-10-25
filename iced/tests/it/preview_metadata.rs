use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr::{diagnostic, host, preview, Engine, ParseErrorType, Shared};

static NEXT_PROJECT: AtomicU64 = AtomicU64::new(0);

#[test]
fn exact_preview_metadata_is_retained_and_deterministic() {
    let engine = compiler();
    let first = engine
        .compile_application_descriptor(valid_source())
        .unwrap();
    let second = engine
        .compile_application_descriptor(valid_source())
        .unwrap();
    let reordered_source = valid_source().replace(
        concat!(
            "#[preview(name = \"Default\", group = \"counter\", themes = [State::theme, State::dark], snapshot = true, size = (640, 480))]\n    ",
            "#[preview(name = \"forty two\", group = \"counter\", seed = State::seed, update = State::update, snapshot = false, size = (320, 240))]"
        ),
        concat!(
            "#[preview(name = \"forty two\", group = \"counter\", seed = State::seed, update = State::update, snapshot = false, size = (320, 240))]\n    ",
            "#[preview(name = \"Default\", group = \"counter\", themes = [State::theme, State::dark], snapshot = true, size = (640, 480))]"
        ),
    );
    let reordered = engine
        .compile_application_descriptor(reordered_source)
        .unwrap();
    let changed = engine
        .compile_application_descriptor(valid_source().replace("(640, 480)", "(641, 480)"))
        .unwrap();

    assert_eq!(first.previews().len(), 2);
    let default = &first.previews()[0];
    assert_eq!(default.key().subject().to_string(), "main::State::view");
    assert_eq!(default.key().variant(), "default");
    assert_eq!(default.name(), "Default");
    assert_eq!(default.group(), "counter");
    assert_eq!(default.size().width(), 640);
    assert_eq!(default.size().height(), 480);
    assert!(default.snapshot());
    assert!(matches!(default.input(), preview::Input::Shared(_)));
    assert!(matches!(default.seed(), preview::Seed::Default));
    assert_eq!(default.update().unwrap().item().key().name(), "update");
    assert_eq!(default.themes().len(), 2);
    assert_eq!(default.locations().themes().len(), 2);
    assert!(default
        .locations()
        .attribute()
        .same_source(default.locations().subject()));
    assert_eq!(default.themes()[0].name(), "theme");
    assert_eq!(default.themes()[1].name(), "dark");

    let seeded = &first.previews()[1];
    assert_eq!(seeded.key().variant(), "forty-two");
    assert!(!seeded.snapshot());
    assert!(matches!(seeded.seed(), preview::Seed::Callback(_)));
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.fingerprint(), reordered.fingerprint());
    assert_ne!(first.fingerprint(), changed.fingerprint());
}

#[test]
fn malformed_preview_signatures_fail_at_compile_time() {
    for (attribute, expected) in [
        ("seed = State::bad_seed", "zero-argument"),
        ("update = State::bad_update", "update must"),
        ("themes = [State::bad_theme]", "theme must"),
    ] {
        let source = valid_source().replace(
            "#[preview(name = \"Default\", group = \"counter\", themes = [State::theme, State::dark], snapshot = true, size = (640, 480))]",
            &format!("#[preview(name = \"Default\", {attribute})]"),
        );
        let error = compiler()
            .compile_application_descriptor(source)
            .unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "expected `{expected}` in {error}"
        );
    }
}

#[test]
fn duplicate_normalized_names_and_invalid_owner_are_compile_errors() {
    let duplicate = valid_source().replace("name = \"forty two\"", "name = \"default\"");
    assert_preview_error(&duplicate, "duplicate normalized preview identity");

    let owner = valid_source().replace("fn view(&self)", "fn view(&mut self)");
    assert_preview_error(&owner, "shared `&self`");
}

#[test]
fn stateless_unit_message_preview_needs_no_update_or_seed() {
    let descriptor = compiler()
        .compile_application_descriptor(stateless_source())
        .unwrap();
    let preview = &descriptor.previews()[0];

    assert!(matches!(preview.input(), preview::Input::Stateless));
    assert!(matches!(preview.seed(), preview::Seed::None));
    assert!(preview.update().is_none());
}

#[test]
fn preview_metadata_compiles_without_an_application_entry() {
    let source = valid_source().replace(
        "pub fn main() -> iced::Result { iced::run(State::update, State::view) }",
        "",
    );
    let executable = compiler().compile_executable(source).unwrap();

    assert_eq!(executable.previews().len(), 2);
}

#[test]
fn shared_and_owned_function_subjects_retain_exact_input_modes() {
    let descriptor = compiler()
        .compile_application_descriptor(function_subject_source())
        .unwrap();

    assert_eq!(descriptor.previews().len(), 2);
    assert!(matches!(
        descriptor.previews()[0].input(),
        preview::Input::Owned(_)
    ));
    assert!(matches!(
        descriptor.previews()[1].input(),
        preview::Input::Shared(_)
    ));
}

#[test]
fn projected_widget_output_survives_descriptor_launch_and_binding() {
    let engine = Shared::new(compiler());
    let descriptor = engine
        .compile_application_descriptor(projected_output_source())
        .unwrap();
    assert_eq!(descriptor.previews().len(), 1);

    let launch = engine.launch_application(descriptor).unwrap();
    assert_eq!(launch.previews().len(), 1);
    let mut runtime = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        runtime.install(facade).unwrap();
    }
    let bound = launch.bind(runtime).unwrap();
    assert_eq!(bound.previews().len(), 1);
}

#[test]
fn host_member_schema_has_stable_digest_evidence() {
    let executable = compiler()
        .compile_executable(
            r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;

struct State { theme: Theme }
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn seed() -> State { State { theme: Theme::Light } }
    fn update(&mut self, message: Message) {}

    #[preview(seed = State::seed)]
    fn view(&self) -> Element<'_, Message> { text("host member") }
}
"#,
        )
        .unwrap();

    assert!(executable.previews()[0].digest().is_ok());
}

#[test]
fn package_local_method_preview_uses_imported_stable_items() {
    let project = Project::new("package-preview");
    project.write("Cargo.toml", "[package]\nname = \"preview-app\"\n");
    project.write(
        "src/main.rs",
        r#"
pub mod component;
use crate::component::State;
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
    );
    project.write(
        "src/component.rs",
        &package_component("crate::component::State::seed"),
    );
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let descriptor = compiler()
        .compile_package_application_descriptor(&target)
        .unwrap();

    assert_eq!(descriptor.previews().len(), 1);
    let preview = &descriptor.previews()[0];
    // The package is named as rustc names the crate (D471).
    assert_eq!(preview.key().subject().package(), "preview_app");
    assert_eq!(preview.key().subject().modules(), ["component"]);
    assert_eq!(
        preview.seed().callback().unwrap().item().key().name(),
        "seed"
    );
}

#[test]
fn package_callback_failure_reports_primary_and_related_sources() {
    let project = Project::new("package-preview-error");
    project.write("Cargo.toml", "[package]\nname = \"preview-app\"\n");
    project.write(
        "src/main.rs",
        r#"
pub mod component;
pub mod fixtures;
use crate::component::State;
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
    );
    project.write(
        "src/component.rs",
        &package_component("crate::fixtures::bad_seed"),
    );
    project.write(
        "src/fixtures.rs",
        "pub fn bad_seed(value: i64) -> i64 { value }\n",
    );
    let target = rawr::project::Target::discover(project.path()).unwrap();

    let error = compiler()
        .compile_package_application_descriptor(&target)
        .unwrap_err();
    let report = error.report().unwrap();

    assert_eq!(report.code(), diagnostic::code::RESOLVE_PREVIEW);
    assert_eq!(report.labels().len(), 2);
    assert!(report.labels()[0]
        .source()
        .identity()
        .ends_with("src/component.rs"));
    assert!(report.labels()[1]
        .source()
        .identity()
        .ends_with("src/fixtures.rs"));
}

#[test]
fn owner_message_default_and_subject_output_are_checked() {
    let owner = valid_source().replace("fn view(&self)", "fn view(&mut self)");
    assert_preview_error(&owner, "shared `&self`");

    let no_default = valid_source()
        .replace("#[derive(Default)]\nstruct State", "struct State")
        .replace(
            "#[preview(name = \"Default\", group = \"counter\", themes = [State::theme, State::dark], snapshot = true, size = (640, 480))]",
            "#[preview(name = \"Default\")]",
        );
    assert_preview_error(&no_default, "does not provide `Default`");

    let wrong_output = valid_source().replace(
        "fn view(&self) -> Element<'_, Message>",
        "fn view(&self) -> i64",
    );
    assert_preview_error(&wrong_output, "not compatible");

    let message = stateless_source()
        .replace(
            "Element<'_, ()> { screen() }",
            "Element<'_, MissingClone> { screen() }",
        )
        .replace(
            "#[preview]",
            "#[derive(Debug)] enum MissingClone { Tick }\n\n#[preview]",
        )
        .replace(
            "Element<'_, ()> { text(\"static\") }",
            "Element<'_, MissingClone> { text(\"static\") }",
        );
    assert_preview_error(&message, "does not provide `Clone`");

    let unknown = valid_source().replace("seed = State::seed", "seed = State::missing");
    assert_preview_error(&unknown, "unknown or inaccessible");
}

#[test]
fn malformed_size_and_module_subject_are_compile_errors() {
    let size = valid_source().replace("size = (640, 480)", "size = (0, 480)");
    let error = compiler().compile_application_descriptor(size).unwrap_err();
    assert!(
        error.to_string().contains("positive u32 literal"),
        "{error}"
    );

    let project = Project::new("module-preview");
    project.write("Cargo.toml", "[package]\nname = \"preview-app\"\n");
    project.write("src/main.rs", "#[preview]\nmod component;\n");
    project.write("src/component.rs", "");
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let error = compiler().compile_package_executable(&target).unwrap_err();
    assert!(error.to_string().contains("only on functions and methods"));
}

fn assert_preview_error(source: &str, expected: &str) {
    let error = compiler()
        .compile_application_descriptor(source)
        .unwrap_err();
    assert!(
        matches!(
            error.err_type(),
            ParseErrorType::Resolution(rawr::resolve::Error::InvalidPreview { .. })
        ),
        "unexpected error: {error:?}"
    );
    assert!(error.to_string().contains(expected), "{error}");
    let report = diagnostic::Report::from_parse(
        diagnostic::Source::new("preview.rs", source).unwrap(),
        &error,
    )
    .unwrap();
    assert_eq!(report.code(), diagnostic::code::RESOLVE_PREVIEW);
}

fn compiler() -> Engine {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine
}

fn valid_source() -> &'static str {
    r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;

#[derive(Default)]
struct State { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Tick }

impl State {
    fn seed() -> State { State { value: 42 } }
    fn bad_seed(value: i64) -> State { State { value } }
    fn update(&mut self, message: Message) {}
    fn bad_update(&self, message: Message) {}
    fn theme(&self) -> Theme { Theme::Light }
    fn dark() -> Theme { Theme::Dark }
    fn bad_theme(&self) -> i64 { 1 }

    #[preview(name = "Default", group = "counter", themes = [State::theme, State::dark], snapshot = true, size = (640, 480))]
    #[preview(name = "forty two", group = "counter", seed = State::seed, update = State::update, snapshot = false, size = (320, 240))]
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn stateless_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;

impl State {
    fn update(&mut self, message: ()) {}
    fn view(&self) -> Element<'_, ()> { screen() }
}

#[preview]
pub fn screen() -> Element<'_, ()> { text("static") }

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn function_subject_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[derive(Default, Clone)] struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { shared_view(self) }
}

#[preview(name = "owned")]
pub fn owned_view(state: State) -> Element<'_, Message> { text(state.value) }

#[preview(name = "shared")]
pub fn shared_view(state: &State) -> Element<'_, Message> { text(state.value) }

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn projected_output_source() -> &'static str {
    r#"
use iced::widget::{Column, column, text};
#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    #[preview]
    fn view(&self) -> Column<'_, Message> { column![text("projected")] }
}
pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn package_component(seed: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::text;
#[derive(Default)] pub struct State {{ value: i64 }}
#[derive(Debug, Clone, Copy)] pub enum Message {{ Tick }}
impl State {{
    pub fn seed() -> State {{ State {{ value: 42 }} }}
    pub fn update(&mut self, message: Message) {{}}
    #[preview(name = "package", seed = {seed})]
    pub fn view(&self) -> Element<'_, Message> {{ text(self.value) }}
}}
"#
    )
}

trait SeedExt {
    fn callback(&self) -> Option<&preview::Callback>;
}

impl SeedExt for preview::Seed {
    fn callback(&self) -> Option<&preview::Callback> {
        match self {
            Self::Callback(callback) => Some(callback),
            Self::None | Self::Default => None,
        }
    }
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("rawr-preview-{}-{id}-{name}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.root.join(path);
        // A `.rs` file is a Rawr file: it starts with `// rawr`.
        let marked;
        let source = if path.extension().is_some_and(|extension| extension == "rs") {
            marked = format!("// rawr\n{}", source);
            marked.as_str()
        } else {
            source
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
