use rawr::application::{Boot, Category, Task};
use rawr::{capability, host, program, receiver, resolve, Engine, ParseErrorType, Shared};

#[test]
fn exact_default_counter_promotes_to_a_normalized_descriptor() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let descriptor = engine
        .compile_application_descriptor(default_source("()"))
        .unwrap();

    assert!(matches!(descriptor.boot(), Boot::Default));
    assert_eq!(
        descriptor.update().callback().receiver(),
        Some(receiver::Mode::Mutable)
    );
    assert!(matches!(descriptor.update().task(), Task::None));
    assert_eq!(
        descriptor.view().callback().receiver(),
        Some(receiver::Mode::Shared)
    );
    assert_eq!(
        descriptor.view().projection().unwrap().target(),
        "iced::Element"
    );
    assert!(descriptor.message().provides(capability::Kind::Clone));
    assert!(descriptor.message().provides(capability::Kind::Send));
    assert!(descriptor.message().provides(capability::Kind::Static));
}

#[test]
fn candidate_validation_consumes_the_exact_executable_bundle() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let candidate = engine.compile_application(default_source("()")).unwrap();
    let generation = candidate.generation();

    let descriptor = candidate.validate().unwrap();
    assert_eq!(descriptor.generation(), generation);
    assert_eq!(descriptor.update().callback().item().key().name(), "update");
}

#[test]
fn explicit_boot_and_task_outputs_normalize_without_runtime_tasks() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    let state_boot = engine
        .compile_application_descriptor(explicit_source("Counter", "()"))
        .unwrap();
    assert!(matches!(
        state_boot.boot(),
        Boot::Explicit {
            task: Task::None,
            ..
        }
    ));
    assert!(matches!(state_boot.update().task(), Task::None));

    let task_boot = engine
        .compile_application_descriptor(explicit_source(
            "(Counter, iced::Task<Message>)",
            "iced::Task<Message>",
        ))
        .unwrap();
    assert!(matches!(
        task_boot.boot(),
        Boot::Explicit {
            task: Task::Facade(_),
            ..
        }
    ));
    assert!(matches!(task_boot.update().task(), Task::Facade(_)));
    assert!(task_boot.view().projection().is_none());

    let free_boot = engine
        .compile_application_descriptor(free_boot_source())
        .unwrap();
    assert!(matches!(
        free_boot.boot(),
        Boot::Explicit {
            task: Task::None,
            ..
        }
    ));
}

#[test]
fn title_and_centered_shell_settings_are_exact_contract_proofs() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let descriptor = engine
        .compile_application_descriptor(shell_source("self.label.clone()", true))
        .unwrap();

    let title = descriptor.title().unwrap();
    assert_eq!(title.callback().receiver(), Some(receiver::Mode::Shared));
    assert_eq!(title.callback().parameters(), &[]);
    assert_eq!(title.callback().output().display_name(), "String");
    assert!(descriptor.centered());

    let changed_body = engine
        .compile_application_descriptor(shell_source("\"Changed\".to_string()", true))
        .unwrap();
    assert_eq!(descriptor.fingerprint(), changed_body.fingerprint());

    let not_centered = engine
        .compile_application_descriptor(shell_source("self.label.clone()", false))
        .unwrap();
    assert_ne!(descriptor.fingerprint(), not_centered.fingerprint());
}

#[test]
fn title_requires_the_exact_shared_state_to_string_contract() {
    for (declaration, callback, expected) in [
        (
            "fn title(&mut self) -> String { \"title\" }",
            "State::title",
            Category::InvalidReceiver,
        ),
        (
            "fn title(&self, value: i64) -> String { \"title\" }",
            "State::title",
            Category::InvalidArity,
        ),
        (
            "fn title(&self) -> bool { true }",
            "State::title",
            Category::InvalidOutput,
        ),
        (
            "fn title(&self) -> String { \"other\" }",
            "Other::title",
            Category::TypeMismatch,
        ),
    ] {
        let source = invalid_title_source(declaration, callback);
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine.compile_application_descriptor(source).unwrap_err();
        assert_application(error.err_type(), expected);
        assert!(error.to_string().contains("title"));
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn explicit_boot_kind_owner_arity_state_and_task_message_are_exact() {
    let cases = [
        (
            explicit_declarations(
                "Counter",
                "fn boot(&self) -> Counter { panic!(\"phase 4\") }",
                "Counter::boot",
            ),
            Category::InvalidCallbackKind,
        ),
        (
            explicit_declarations(
                "Counter",
                "fn boot(value: i64) -> Counter { panic!(\"phase 4\") }",
                "Counter::boot",
            ),
            Category::InvalidArity,
        ),
        (
            explicit_declarations(
                "Counter",
                "fn boot() -> OtherState { panic!(\"phase 4\") }",
                "Counter::boot",
            ),
            Category::TypeMismatch,
        ),
        (
            explicit_declarations(
                "Counter",
                "fn boot() -> (Counter, iced::Task<Other>) { panic!(\"phase 4\") }",
                "Counter::boot",
            ),
            Category::InvalidOutput,
        ),
        (
            explicit_declarations(
                "OtherState",
                "fn boot() -> Counter { panic!(\"phase 4\") }",
                "OtherState::boot",
            ),
            Category::TypeMismatch,
        ),
    ];

    for (source, expected) in cases {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine.compile_application_descriptor(source).unwrap_err();
        assert_application(error.err_type(), expected);
        assert!(!error.primary_span().is_none());
        assert!(error.to_string().contains("boot"));
    }
}

#[test]
fn callback_state_message_receiver_kind_arity_and_output_are_exact() {
    let cases = [
        (
            declarations(
                "fn update(&self, message: Message) { () }",
                "fn view(&self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
                true,
            ),
            Category::InvalidReceiver,
        ),
        (
            declarations(
                "fn update(&mut self, message: Message) { () }",
                "fn view(&mut self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
                true,
            ),
            Category::InvalidReceiver,
        ),
        (
            declarations(
                "fn update(message: Message) { () }",
                "fn view(&self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
                true,
            ),
            Category::InvalidCallbackKind,
        ),
        (
            declarations(
                "fn update(&mut self) { () }",
                "fn view(&self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
                true,
            ),
            Category::InvalidArity,
        ),
        (
            declarations(
                "fn update(&mut self, message: Message) -> Counter { panic!(\"phase 4\") }",
                "fn view(&self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
                true,
            ),
            Category::InvalidOutput,
        ),
        (mismatched_state_source(), Category::TypeMismatch),
        (
            declarations(
                "fn update(&mut self, message: Message) { () }",
                "fn view(&self) -> iced::widget::Column<'_, Other> { panic!(\"phase 4\") }",
                true,
            ),
            Category::InvalidOutput,
        ),
    ];

    for (source, expected) in cases {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine.compile_application_descriptor(source).unwrap_err();
        assert_application(error.err_type(), expected);
        assert!(!error.primary_span().is_none());
        assert!(error.position().line().is_some());
    }
}

#[test]
fn default_and_message_capabilities_are_proved_before_descriptor_exists() {
    let mut no_default = Engine::new();
    rawr_iced::install(&mut no_default).unwrap();
    let error = no_default
        .compile_application_descriptor(declarations(
            "fn update(&mut self, message: Message) { () }",
            "fn view(&self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
            false,
        ))
        .unwrap_err();
    assert_application(error.err_type(), Category::MissingDefault);

    let mut no_clone = Engine::new();
    rawr_iced::install(&mut no_clone).unwrap();
    let error = no_clone
        .compile_application_descriptor(default_source_with_message("#[derive(Debug)]"))
        .unwrap_err();
    assert_application(error.err_type(), Category::MissingCapability);
    assert!(error.to_string().contains("Clone"));

    for (missing, provided) in [
        (
            "Send",
            [capability::Kind::Clone, capability::Kind::Static].as_slice(),
        ),
        (
            "Static",
            [capability::Kind::Clone, capability::Kind::Send].as_slice(),
        ),
    ] {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        engine
            .register_host_facade(opaque_facade(provided))
            .unwrap();
        let error = engine
            .compile_application_descriptor(nested_host_message_source())
            .unwrap_err();
        assert_application(error.err_type(), Category::MissingCapability);
        assert!(error.to_string().contains(missing));
    }
}

#[test]
fn third_party_widget_output_uses_authenticated_cross_facade_projection() {
    let iced = rawr_iced::facade().unwrap();
    let target = iced.export_type("Element").unwrap();
    let mut charts =
        host::facade::Builder::new("charts", host::facade::Abi::new(1).unwrap()).unwrap();
    charts.type_::<Plot>("Plot", &["Message"], &[]).unwrap();
    charts.project_to("Plot", target, &["Message"]).unwrap();

    let mut engine = Engine::new();
    engine.register_host_facade(iced).unwrap();
    engine
        .register_host_facade(charts.finish().unwrap())
        .unwrap();
    let descriptor = engine
        .compile_application_descriptor(cross_facade_source())
        .unwrap();
    assert_eq!(
        descriptor.view().projection().unwrap().target(),
        "iced::Element"
    );
}

#[test]
fn fingerprints_are_deterministic_and_cover_schema_and_callback_contracts() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let first = engine
        .compile_application_descriptor(default_source("()"))
        .unwrap();
    let second = engine
        .compile_application_descriptor(default_source("()"))
        .unwrap();
    assert_ne!(first.generation(), second.generation());
    assert_eq!(first.fingerprint(), second.fingerprint());

    let schema = engine
        .compile_application_descriptor(default_source_with_state("value: i64, other: bool"))
        .unwrap();
    assert_ne!(first.fingerprint(), schema.fingerprint());

    let task = engine
        .compile_application_descriptor(default_source("iced::Task<Message>"))
        .unwrap();
    assert_ne!(first.fingerprint(), task.fingerprint());

    let reordered = engine
        .compile_application_descriptor(reordered_default_source())
        .unwrap();
    assert_eq!(first.fingerprint(), reordered.fingerprint());

    let stronger_message = engine
        .compile_application_descriptor(default_source_with_message("#[derive(Clone, Copy)]"))
        .unwrap();
    assert_ne!(first.fingerprint(), stronger_message.fingerprint());

    let free_boot = engine
        .compile_application_descriptor(free_boot_source())
        .unwrap();
    let associated_boot = engine
        .compile_application_descriptor(explicit_source("Counter", "()"))
        .unwrap();
    assert_ne!(free_boot.fingerprint(), associated_boot.fingerprint());
}

#[test]
fn descriptor_fingerprint_captures_exact_facade_abi() {
    let mut first = Engine::new();
    first.register_host_facade(application_facade(1)).unwrap();
    let first = first
        .compile_application_descriptor(default_source("()"))
        .unwrap();

    let mut second = Engine::new();
    second.register_host_facade(application_facade(2)).unwrap();
    let second = second
        .compile_application_descriptor(default_source("()"))
        .unwrap();

    assert_ne!(first.facade(), second.facade());
    assert_ne!(first.fingerprint(), second.fingerprint());
}

#[test]
fn descriptor_requires_canonical_task_and_widget_facade_types() {
    for (include_task, include_widget, required) in [
        (false, true, "iced::Task<Message>"),
        (true, false, "iced::Element<Message>"),
    ] {
        let mut engine = Engine::new();
        engine
            .register_host_facade(application_facade_contract(1, include_task, include_widget))
            .unwrap();
        let error = engine
            .compile_application_descriptor(default_source("()"))
            .unwrap_err();
        assert_application(error.err_type(), Category::InvalidFacadeContract);
        assert!(error.to_string().contains(required));
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn runtime_message_graphs_reject_structural_tuples() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    engine
        .compile_application_descriptor(nested_tuple_state_source())
        .expect("State holds tuples, as in Rust");
    for (source, role) in [
        (direct_tuple_message_source(), "message"),
        (nested_tuple_message_source(), "message"),
    ] {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let error = engine.compile_application_descriptor(source).unwrap_err();
        assert_application(error.err_type(), Category::UnsupportedRuntimeType);
        assert!(error.to_string().contains(role));
        assert!(error.to_string().contains("structural tuple"));
        assert!(!error.primary_span().is_none());
    }
}

#[test]
fn descriptor_launch_requires_the_exact_compiler_engine_origin() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let descriptor = engine
        .compile_application_descriptor(default_source("()"))
        .unwrap();
    let fingerprint = descriptor.fingerprint().clone();
    let generation = descriptor.generation();
    let launch = engine.launch_application(descriptor).unwrap();
    assert_eq!(launch.fingerprint(), &fingerprint);
    assert_eq!(launch.authority().generation(), generation);

    let descriptor = engine
        .compile_application_descriptor(default_source("()"))
        .unwrap();
    let mut other = Engine::new();
    rawr_iced::install(&mut other).unwrap();
    let other = Shared::new(other);
    assert!(matches!(
        other.launch_application(descriptor),
        Err(program::Error::EngineOriginMismatch)
    ));
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

fn default_source(update_output: &str) -> String {
    let update = if update_output == "()" {
        "fn update(&mut self, message: Message) { () }".to_string()
    } else {
        format!("fn update(&mut self, message: Message) -> {update_output} {{ panic!(\"phase 4\") }}")
    };
    declarations(
        &update,
        "fn view(&self) -> iced::widget::Column<'_, Message> { panic!(\"phase 4\") }",
        true,
    )
}

fn default_source_with_message(attribute: &str) -> String {
    format!(
        r#"
#[derive(Default)]
struct Counter {{ value: i64 }}
{attribute}
enum Message {{ Tick }}
impl Counter {{
    fn update(&mut self, message: Message) {{ () }}
    fn view(&self) -> iced::widget::Column<'_, Message> {{ panic!("phase 4") }}
}}
pub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}
"#
    )
}

fn default_source_with_state(fields: &str) -> String {
    format!(
        r#"
#[derive(Default)] struct Counter {{ {fields} }}
#[derive(Clone)] enum Message {{ Tick }}
impl Counter {{
    fn update(&mut self, message: Message) {{ () }}
    fn view(&self) -> iced::widget::Column<'_, Message> {{ panic!("phase 4") }}
}}
pub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}
"#
    )
}

fn explicit_source(boot_output: &str, update_output: &str) -> String {
    let update = if update_output == "()" {
        "fn update(&mut self, message: Message) { () }".to_string()
    } else {
        format!("fn update(&mut self, message: Message) -> {update_output} {{ panic!(\"phase 4\") }}")
    };
    format!(
        r#"
struct Counter {{ value: i64 }}
#[derive(Clone)] enum Message {{ Tick }}
impl Counter {{
    fn new() -> {boot_output} {{ panic!("phase 4") }}
    {update}
    fn view(&self) -> iced::Element<Message> {{ panic!("phase 4") }}
}}
pub fn main() -> iced::Result {{
    iced::application(Counter::new, Counter::update, Counter::view).run()
}}
"#
    )
}

fn shell_source(title: &str, centered: bool) -> String {
    let centered = if centered { ".centered()" } else { "" };
    format!(
        r#"
use iced::Element;
use iced::widget::text;
struct State {{ label: String }}
#[derive(Clone)] enum Message {{ Tick }}
impl State {{
    fn new() -> State {{ State {{ label: "Tour".to_string() }} }}
    fn update(&mut self, message: Message) {{ () }}
    fn view(&self) -> Element<Message> {{ text(&self.label) }}
    fn title(&self) -> String {{ {title} }}
}}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view)
        .title(State::title){centered}
        .run()
}}
"#,
    )
}

fn invalid_title_source(declaration: &str, callback: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::text;
struct State;
struct Other;
#[derive(Clone)] enum Message {{ Tick }}
impl State {{
    fn new() -> State {{ State {{}} }}
    fn update(&mut self, message: Message) {{ () }}
    fn view(&self) -> Element<Message> {{ text("state") }}
    {declaration}
}}
impl Other {{ {declaration} }}
pub fn main() -> iced::Result {{
    iced::application(State::new, State::update, State::view)
        .title({callback})
        .run()
}}
"#,
    )
}

fn free_boot_source() -> &'static str {
    r#"
struct Counter { value: i64 }
#[derive(Clone)] enum Message { Tick }
pub fn boot() -> Counter { panic!("phase 4") }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> iced::Element<Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result {
    iced::application(boot, Counter::update, Counter::view).run()
}
"#
}

fn explicit_declarations(boot_owner: &str, boot: &str, callback: &str) -> String {
    format!(
        r#"
struct Counter;
struct OtherState;
#[derive(Clone)] enum Message {{ Tick }}
#[derive(Clone)] enum Other {{ Tick }}
impl {boot_owner} {{ {boot} }}
impl Counter {{
    fn update(&mut self, message: Message) {{ () }}
    fn view(&self) -> iced::Element<Message> {{ panic!("phase 4") }}
}}
pub fn main() -> iced::Result {{
    iced::application({callback}, Counter::update, Counter::view).run()
}}
"#
    )
}

fn declarations(update: &str, view: &str, derive_default: bool) -> String {
    let derive = if derive_default {
        "#[derive(Default)]"
    } else {
        ""
    };
    format!(
        r#"
{derive}
struct Counter {{ value: i64 }}
#[derive(Clone)] enum Message {{ Tick }}
enum Other {{ Tick }}
impl Counter {{
    {update}
    {view}
}}
pub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}
"#
    )
}

fn reordered_default_source() -> &'static str {
    r#"
#[derive(Clone)] enum Message { Tick }
#[derive(Default)] struct Counter { value: i64 }
impl Counter {
    fn view(&self) -> iced::widget::Column<'_, Message> { panic!("phase 4") }
    fn update(&mut self, message: Message) { () }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn mismatched_state_source() -> String {
    r#"
#[derive(Default)] struct Counter;
struct OtherState;
#[derive(Clone)] enum Message { Tick }
impl Counter { fn update(&mut self, message: Message) { () } }
impl OtherState { fn view(&self) -> iced::widget::Column<'_, Message> { panic!("phase 4") } }
pub fn main() -> iced::Result { iced::run(Counter::update, OtherState::view) }
"#
    .to_string()
}

fn direct_tuple_message_source() -> &'static str {
    r#"
#[derive(Default)] struct Counter;
impl Counter {
    fn update(&mut self, message: (i64, bool)) { () }
    fn view(&self) -> iced::widget::Column<'_, (i64, bool)> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn nested_tuple_message_source() -> &'static str {
    r#"
#[derive(Default)] struct Counter;
#[derive(Clone)] struct Payload { pair: (i64, bool) }
#[derive(Clone)] enum Message { Data(Payload) }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> iced::widget::Column<'_, Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn nested_tuple_state_source() -> &'static str {
    r#"
#[derive(Default)] struct Counter { pair: (i64, bool) }
#[derive(Clone)] enum Message { Tick }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> iced::widget::Column<'_, Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn opaque_facade(capabilities: &[capability::Kind]) -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("native", host::facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<Opaque>("Opaque", &[], capabilities)
        .unwrap();
    builder.finish().unwrap()
}

fn application_facade(abi: u32) -> host::facade::Definition {
    application_facade_contract(abi, true, true)
}

fn application_facade_contract(
    abi: u32,
    include_task: bool,
    include_widget: bool,
) -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("iced", host::facade::Abi::new(abi).unwrap()).unwrap();
    builder.module("widget").unwrap();
    builder.type_::<FacadeResult>("Result", &[], &[]).unwrap();
    if include_task {
        builder
            .type_::<FacadeTask>("Task", &["Message"], &[])
            .unwrap();
    }
    if include_widget {
        builder
            .type_::<FacadeValue>("Element", &["Message"], &[])
            .unwrap();
    }
    builder
        .type_::<FacadeColumn>("widget::Column", &["Message"], &[])
        .unwrap();
    if include_widget {
        builder
            .project("widget::Column", "Element", &["Message"])
            .unwrap();
    }

    let mut signature = host::signature::Builder::new();
    signature
        .type_parameter("State", &[])
        .unwrap()
        .type_parameter(
            "Message",
            &[
                capability::Kind::Clone,
                capability::Kind::Send,
                capability::Kind::Static,
            ],
        )
        .unwrap()
        .type_parameter("Update", &[])
        .unwrap()
        .type_parameter("View", &[])
        .unwrap();
    signature.parameter(host::type_ref::Type::parameter("Update").unwrap());
    signature.parameter(host::type_ref::Type::parameter("View").unwrap());
    signature.output(host::type_ref::Type::registered("Result").unwrap());
    builder
        .item(host::item::Definition::intrinsic_function("run", signature.finish()).unwrap())
        .unwrap();
    builder.finish().unwrap()
}

fn nested_host_message_source() -> &'static str {
    r#"
use native::Opaque;
#[derive(Default)] struct Counter;
#[derive(Clone)] enum Message { Data(Opaque) }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> iced::widget::Column<'_, Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn cross_facade_source() -> &'static str {
    r#"
#[derive(Default)] struct Counter;
#[derive(Clone)] enum Message { Tick }
impl Counter {
    fn update(&mut self, message: Message) { () }
    fn view(&self) -> charts::Plot<Message> { panic!("phase 4") }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

struct Opaque;
struct Plot;
struct FacadeResult;
struct FacadeTask;
struct FacadeValue;
struct FacadeColumn;
