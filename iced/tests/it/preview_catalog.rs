use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rawr::{capability, diagnostic, host, preview, Engine, Shared};

static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

#[test]
fn one_catalog_binds_its_optional_application_and_executes_previews() {
    let engine = compiler();
    let catalog = engine
        .compile_preview_catalog_source(source("catalog.rs", catalog_source()))
        .unwrap();
    assert!(catalog.has_application());
    assert_eq!(catalog.previews().len(), 1);
    assert_eq!(catalog.previews()[0].name(), "forty two");

    let launch = engine.launch_previews(catalog).unwrap();
    let bound = launch.bind(registry()).unwrap();
    assert_eq!(bound.package(), "main");
    assert!(bound.application().is_some());
    let definition = &bound.previews()[0];
    let key = definition.key().clone();
    let fixture = bound.seed(&key).unwrap().unwrap();
    let widget = bound.subject(&key, Some(&fixture)).unwrap();
    rawr_iced::widget::Value::from_dynamic(widget).unwrap();
    let theme = bound.theme(&key, 0, Some(&fixture)).unwrap();
    let theme = rawr_iced::theme::Value::from_dynamic(theme).unwrap();
    assert_eq!(theme.native(), &iced::Theme::Dark);
}

#[test]
fn standalone_preview_catalog_has_no_synthetic_application() {
    let engine = compiler();
    let catalog = engine
        .compile_preview_catalog_source(source("standalone.rs", standalone_source()))
        .unwrap();

    assert!(!catalog.has_application());
    let launch = engine.launch_previews(catalog).unwrap();
    let bound = launch.bind(registry()).unwrap();
    assert!(bound.application().is_none());
    let key = bound.previews()[0].key().clone();
    assert!(bound.seed(&key).unwrap().is_none());
    let widget = bound.subject(&key, None).unwrap();
    rawr_iced::widget::Value::from_dynamic(widget).unwrap();
}

#[test]
fn catalog_keeps_one_engine_generation_and_rejects_foreign_source_locations() {
    let first = compiler();
    let first_catalog = first
        .compile_preview_catalog_source(source("first.rs", standalone_source()))
        .unwrap();
    let first_location = first_catalog.previews()[0].locations().attribute();
    let first_generation = first_catalog.generation();
    let first_bound = first
        .launch_previews(first_catalog)
        .unwrap()
        .bind(registry())
        .unwrap();
    assert_eq!(first_bound.generation(), first_generation);
    assert_eq!(
        first_bound.source(first_location).unwrap().identity(),
        "first.rs"
    );

    let second = compiler();
    let second_catalog = second
        .compile_preview_catalog_source(source("second.rs", standalone_source()))
        .unwrap();
    let second_bound = second
        .launch_previews(second_catalog)
        .unwrap()
        .bind(registry())
        .unwrap();
    assert!(matches!(
        second_bound.source(first_location),
        Err(preview::RuntimeError::ForeignSource)
    ));

    let foreign_catalog = first
        .compile_preview_catalog_source(source("foreign.rs", standalone_source()))
        .unwrap();
    assert!(second.launch_previews(foreign_catalog).is_err());
}

#[test]
fn catalog_reports_from_retained_source_after_the_package_file_is_deleted() {
    let project = Project::new("retained-catalog-source");
    project.write("src/main.rs", standalone_source());
    let target = rawr::project::Target::discover(project.path()).unwrap();
    let engine = compiler();
    let catalog = engine.compile_package_preview_catalog(&target).unwrap();
    let location = catalog.previews()[0].locations().attribute();

    fs::remove_file(project.path().join("src/main.rs")).unwrap();

    let bound = engine
        .launch_previews(catalog)
        .unwrap()
        .bind(registry())
        .unwrap();
    // The package is named as rustc names the crate (D471).
    assert_eq!(bound.package(), "fixture");
    let retained = bound.source(location).unwrap();
    assert_eq!(retained.text(), format!("// rawr\n{}", standalone_source()));
    assert!(retained.identity().ends_with("src/main.rs"));
}

#[test]
fn preview_only_boundary_is_not_an_application_obligation_but_is_a_catalog_obligation() {
    let mut engine = Engine::new();
    let iced = rawr_iced::facade().unwrap();
    let gallery = gallery_facade(&iced);
    engine.register_host_facade(iced).unwrap();
    engine.register_host_facade(gallery).unwrap();
    let engine = Shared::new(engine);

    let descriptor = engine
        .compile_application_descriptor(adversarial_source())
        .unwrap();
    let application = engine.launch_application(descriptor).unwrap();
    application.bind(registry()).unwrap();

    let catalog = engine
        .compile_preview_catalog_source(source("adversarial.rs", adversarial_source()))
        .unwrap();
    let launch = engine.launch_previews(catalog).unwrap();
    let error = launch.bind(registry()).unwrap_err();
    assert!(matches!(
        error,
        host::runtime::Error::MissingFacade { ref facade } if facade == "gallery"
    ));
}

fn compiler() -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    Shared::new(engine)
}

fn registry() -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
}

fn source(name: &str, text: &str) -> diagnostic::Source {
    diagnostic::Source::new(name, text).unwrap()
}

fn gallery_facade(iced: &host::facade::Definition) -> host::facade::Definition {
    let target = iced.export_type("Element").unwrap();
    let mut builder =
        host::facade::Builder::new("gallery", host::facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<Plot>(
            "Plot",
            &["Message"],
            &[
                capability::Kind::Clone,
                capability::Kind::Send,
                capability::Kind::Static,
            ],
        )
        .unwrap();
    builder.project_to("Plot", target, &["Message"]).unwrap();
    builder.finish().unwrap()
}

#[derive(Clone)]
struct Plot;

fn catalog_source() -> &'static str {
    r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;

#[derive(Default)]
struct State { value: i64 }

#[derive(Debug, Clone, Copy)]
enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    fn dark(&self) -> Theme { Theme::Dark }

    #[preview(name = "forty two", themes = [State::dark])]
    fn view(&self) -> Element<'_, Message> { text(self.value + 42) }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

fn standalone_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[preview(name = "standalone")]
pub fn sample() -> Element<'_, ()> { text("standalone") }
"#
}

fn adversarial_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("application") }
}

#[preview]
fn native_preview() -> gallery::Plot<()> { panic!("preview body is not executed") }

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
}

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let id = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rawr-preview-catalog-{}-{id}-{name}",
            std::process::id()
        ));
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
