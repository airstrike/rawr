use rawr::application::{Category, Error, Form};
use rawr::{receiver, resolve, Dynamic, Engine, ParseErrorType, Scope, Shared};

#[test]
fn resolves_both_restricted_application_roots_to_stable_items() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    let default = engine
        .compile_application(source("iced::run(Counter::update, Counter::view)"))
        .unwrap();
    assert_eq!(default.entry().form(), Form::Run);
    assert!(default.entry().boot().is_none());
    assert_eq!(
        default.entry().update().receiver(),
        Some(receiver::Mode::Mutable)
    );
    assert_eq!(
        default.entry().view().receiver(),
        Some(receiver::Mode::Shared)
    );
    assert_eq!(
        default.entry().update().key().to_string(),
        "main::Counter::update"
    );
    assert_eq!(
        default.entry().view().key().to_string(),
        "main::Counter::view"
    );
    assert_eq!(
        default.entry().root_intrinsic(),
        &rawr_iced::facade()
            .unwrap()
            .items()
            .find(|item| item.path() == "iced::run")
            .unwrap()
            .fingerprint()
    );

    let explicit = engine
        .compile_application(source(
            "iced::application(Counter::new, Counter::update, Counter::view).run()",
        ))
        .unwrap();
    assert_eq!(explicit.entry().form(), Form::Application);
    assert_eq!(
        explicit.entry().boot().unwrap().key().to_string(),
        "main::Counter::new"
    );
    assert_eq!(explicit.entry().boot().unwrap().receiver(), None);
    assert!(explicit.entry().run_intrinsic().is_some());
    assert_eq!(
        explicit.entry().facade(),
        rawr_iced::facade().unwrap().fingerprint()
    );

    let shell = engine
        .compile_application(source(
            "iced::application(Counter::new, Counter::update, Counter::view).centered().title(Counter::title).run()",
        ))
        .unwrap();
    assert_eq!(
        shell.entry().title().unwrap().key().to_string(),
        "main::Counter::title"
    );
    assert!(shell.entry().title_intrinsic().is_some());
    assert!(shell.entry().centered_intrinsic().is_some());
    assert!(shell.entry().centered());
}

#[test]
fn application_metadata_is_stable_across_compilation_and_declaration_order() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let first = engine
        .compile_application(source("iced::run(Counter::update, Counter::view)"))
        .unwrap();
    let second = engine
        .compile_application(source_reordered(
            "iced::run(Counter::update, Counter::view)",
        ))
        .unwrap();

    assert_ne!(first.generation(), second.generation());
    assert_eq!(first.entry().form(), second.entry().form());
    assert_eq!(first.entry().facade(), second.entry().facade());
    assert_eq!(
        first.entry().root_intrinsic(),
        second.entry().root_intrinsic()
    );
    assert_eq!(first.entry().update(), second.entry().update());
    assert_eq!(first.entry().view(), second.entry().view());
}

#[test]
fn compiler_owned_main_cannot_escape_the_application_proof_bundle() {
    let script = source("iced::run(Counter::update, Counter::view)");
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    let candidate = engine.compile_application(&script).unwrap();
    assert!(candidate.items().all(|item| item.key().name() != "main"));
    assert!(candidate
        .items()
        .any(|item| item.key().to_string() == "main::Counter::update"));

    let ast = engine.compile(&script).unwrap();
    assert!(ast.iter_functions().all(|function| function.name != "main"));
    let error = engine
        .call_fn::<Dynamic>(&mut Scope::new(), &ast, "main", ())
        .unwrap_err();
    assert!(error.to_string().contains("main"));

    let shared = Shared::new(engine);
    let executable = shared.compile_executable(&script).unwrap();
    assert!(executable.items().all(|item| item.key().name() != "main"));
    let authority = shared.authorize(executable).unwrap();
    assert!(authority.items().all(|item| item.key().name() != "main"));
}

#[test]
fn application_promotion_requires_an_entry_without_changing_general_compilation() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    let executable = engine.compile_executable("pub fn helper() { () }").unwrap();
    assert!(matches!(
        executable.require_application(),
        Err(Error::MissingEntry)
    ));

    let error = engine
        .compile_application("pub fn helper() { () }")
        .unwrap_err();
    assert_application(error.err_type(), Category::MissingEntry);
}

#[test]
fn application_entry_rejects_ambiguous_locations_and_shapes_with_positions() {
    let cases = [
        (
            "fn main() { iced::run(Counter::update, Counter::view) }",
            Category::PrivateMain,
            1,
        ),
        (
            "pub fn main(value: i64) { iced::run(Counter::update, Counter::view) }",
            Category::InvalidMain,
            1,
        ),
        (
            "pub fn main() { if true { iced::run(Counter::update, Counter::view) } }",
            Category::ConditionalEntry,
            1,
        ),
        (
            "pub fn main() { consume(iced::run(Counter::update, Counter::view)) }",
            Category::NestedEntry,
            1,
        ),
        (
            "pub fn main() { iced::run(Counter::update, Counter::view); () }",
            Category::NonTailEntry,
            1,
        ),
        (
            "pub fn helper() { iced::run(Counter::update, Counter::view) } pub fn main() { helper() }",
            Category::HiddenEntry,
            1,
        ),
        (
            "pub fn main() { iced::run(Counter::update, Counter::view); iced::run(Counter::update, Counter::view) }",
            Category::MultipleEntries,
            1,
        ),
        (
            "pub fn main() { iced::run(Counter::update) }",
            Category::MalformedEntry,
            1,
        ),
        (
            "pub fn main() { iced::application(Counter::new, Counter::update, Counter::view) }",
            Category::MalformedEntry,
            1,
        ),
        (
            "pub fn main() { iced::application(Counter::new, Counter::update, Counter::view).title(Counter::title).title(Counter::title).run() }",
            Category::MalformedEntry,
            1,
        ),
        (
            "pub fn main() { iced::application(Counter::new, Counter::update, Counter::view).centered().centered().run() }",
            Category::MalformedEntry,
            1,
        ),
    ];

    for (main, expected, line) in cases {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine
            .compile_executable(format!("{}\n{main}", declarations()))
            .unwrap_err();
        assert_application(error.err_type(), expected);
        assert_eq!(
            error.position().line(),
            Some(line + declarations().lines().count())
        );
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn callbacks_must_be_known_symbolic_script_items() {
    let cases = [
        (
            "iced::run(make_update(), Counter::view)",
            Category::NonSymbolicCallback,
        ),
        (
            "iced::run(Counter::missing, Counter::view)",
            Category::UnknownCallback,
        ),
        ("iced::run(main, Counter::view)", Category::UnknownCallback),
        (
            "iced::run(42, Counter::view)",
            Category::NonSymbolicCallback,
        ),
    ];

    for (entry, expected) in cases {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine.compile_executable(source(entry)).unwrap_err();
        assert_application(error.err_type(), expected);
        assert!(error.to_string().contains("callback"));
        assert!(error.position().line().is_some());
    }
}

#[test]
fn ordinary_functions_named_run_and_application_are_not_framework_roots() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let executable = engine
        .compile_executable(
            r#"
pub fn run() { () }
pub fn application() { () }
pub fn main() { run(); application() }
"#,
        )
        .unwrap();

    assert!(matches!(
        executable.require_application(),
        Err(Error::MissingEntry)
    ));
}

#[test]
fn multiple_entry_diagnostic_retains_both_exact_spans() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let source = format!(
        "{}\npub fn main() {{\n    iced::run(Counter::update, Counter::view);\n    iced::run(Counter::update, Counter::view)\n}}",
        declarations()
    );
    let error = engine.compile_executable(source).unwrap_err();

    assert_application(error.err_type(), Category::MultipleEntries);
    assert_eq!(error.primary_span().start().line(), Some(11));
    assert_eq!(error.primary_span().start().position(), Some(5));
    assert_eq!(error.secondary_span().unwrap().start().line(), Some(10));
    assert_eq!(error.secondary_span().unwrap().start().position(), Some(5));
}

fn assert_application(error: &ParseErrorType, category: Category) {
    assert!(
        matches!(
            error,
            ParseErrorType::Resolution(resolve::Error::Application(error))
                if error.category() == category
        ),
        "expected {category:?}, found {error:?}"
    );
}

fn source(entry: &str) -> String {
    format!(
        "{}\npub fn main() -> iced::Result {{ {entry} }}",
        declarations()
    )
}

fn source_reordered(entry: &str) -> String {
    format!(
        r#"enum Message {{ Tick }}
struct Counter;
impl Counter {{
    fn view(&self) {{ () }}
    fn update(&mut self, message: Message) {{ () }}
    fn new() -> Counter {{ Counter {{}} }}
}}
pub fn main() -> iced::Result {{ {entry} }}"#
    )
}

fn declarations() -> &'static str {
    r#"struct Counter;
enum Message { Tick }
impl Counter {
    fn new() -> Counter { Counter {} }
    fn update(&mut self, message: Message) { () }
    fn view(&self) { () }
    fn title(&self) -> String { "Counter" }
}"#
}

#[test]
fn application_boots_from_a_derived_default() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let derived = format!(
        "#[derive(Default)]\n{}\npub fn main() -> iced::Result {{ iced::application(Counter::default, Counter::update, Counter::view).run() }}",
        declarations()
    );
    let application = engine.compile_application(derived).unwrap();
    assert_eq!(application.entry().form(), Form::Application);
    assert!(application.entry().boot().is_none(), "boots from Default");

    let missing = engine
        .compile_executable(source(
            "iced::application(Counter::missing, Counter::update, Counter::view).run()",
        ))
        .unwrap_err();
    assert_application(missing.err_type(), Category::UnknownCallback);
}

#[test]
fn run_boots_from_a_hand_written_default() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let written = format!(
        "{}\nimpl Default for Counter {{ fn default() -> Self {{ Counter::new() }} }}\npub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}",
        declarations()
    );
    let application = engine.compile_application(written).unwrap();
    assert_eq!(application.entry().form(), Form::Run);
    assert!(application.entry().boot().is_some(), "runs `Default::default`");

    let derived = format!(
        "#[derive(Default)]\n{}\npub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}",
        declarations()
    );
    let application = engine.compile_application(derived).unwrap();
    assert!(application.entry().boot().is_none(), "boots from the derived Default");
}
