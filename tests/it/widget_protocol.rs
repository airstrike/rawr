#![cfg(not(feature = "no_function"))]

use rawr::diagnostic::{Report, Source, code};
use rawr::host;
use rawr::widget::{self, hook, message, state};
use rawr::{Engine, ParseErrorType, resolve};

#[test]
fn marked_impl_proves_the_complete_renderer_neutral_protocol() {
    let source = r#"
        struct State { clicks: i64 }
        struct Dial { radius: f64 }
        enum Message { Changed(i64) }

        #[widget]
        impl Dial {
            fn state(&self) -> State { panic!("state") }
            fn children(&self) -> rawr_iced::widget::children::List<Message> { panic!("children") }
            fn diff(&mut self, state: &mut State) { state.clicks += 1; }
            fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
            fn layout(&mut self, state: &mut State, input: rawr_iced::widget::layout::Input) -> rawr_iced::widget::layout::Node { panic!("layout") }
            fn update(&mut self, state: &mut State, input: rawr_iced::widget::event::Input) -> rawr_iced::widget::event::Outcome<Message> { panic!("update") }
            fn draw(&self, state: &State, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) { renderer.fill() }
            fn mouse_interaction(&self, state: &State, input: rawr_iced::widget::mouse::Input) -> iced::mouse::Interaction { panic!("mouse") }
            fn operate(&mut self, state: &mut State, input: rawr_iced::widget::operation::Input) -> rawr_iced::widget::operation::Outcome { panic!("operate") }
            fn overlay(&mut self, state: &mut State, input: rawr_iced::widget::overlay::Input) -> rawr_iced::widget::overlay::Value<Message> { panic!("overlay") }
        }
    "#;
    let executable = compiler().compile_executable(source).unwrap();
    let [protocol] = executable.widgets() else {
        panic!("expected one widget protocol");
    };

    assert_eq!(protocol.state().nominal_type().unwrap().display_name(), "main::State");
    assert_eq!(protocol.message().exact().unwrap().display_name(), "main::Message");
    assert_eq!(protocol.dependency().root(), "iced");
    assert_eq!(protocol.lifecycle().root(), "rawr_iced");
    assert_eq!(
        protocol.hooks().iter().map(hook::Definition::kind).collect::<Vec<_>>(),
        [
            hook::Kind::State,
            hook::Kind::Children,
            hook::Kind::Diff,
            hook::Kind::Size,
            hook::Kind::Layout,
            hook::Kind::Update,
            hook::Kind::Draw,
            hook::Kind::MouseInteraction,
            hook::Kind::Operate,
            hook::Kind::Overlay,
        ]
    );
    assert!(protocol.declaration().same_source(protocol.hooks()[0].location()));
}

#[test]
fn omitted_hooks_retain_exact_defaults_and_generic_message() {
    let executable = compiler()
        .compile_executable(
            r#"
                struct Dial;
                #[widget]
                impl Dial {
                    fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
                }
            "#,
        )
        .unwrap();
    let protocol = &executable.widgets()[0];

    assert!(matches!(protocol.state(), state::Type::Unit));
    assert!(matches!(protocol.message(), message::Type::Generic));
    assert!(protocol.hook(hook::Kind::Update).is_none());
    assert_eq!(hook::Kind::Update.default_behavior(), hook::Default::IgnoredEvent);
    assert_eq!(hook::Kind::Children.default_behavior(), hook::Default::EmptyChildren);
    assert_eq!(hook::Kind::Layout.default_behavior(), hook::Default::ZeroLayout);
}

#[test]
fn canonical_fingerprint_ignores_hook_source_order() {
    let first = compile_fingerprint(
        r#"
            struct Dial;
            #[widget]
            impl Dial {
                fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) { renderer.fill() }
            }
        "#,
    );
    let second = compile_fingerprint(
        r#"
            struct Dial;
            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) { renderer.fill() }
                fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
            }
        "#,
    );

    assert_eq!(first, second);
}

#[test]
fn unsupported_hooks_and_inconsistent_messages_are_compile_diagnostics() {
    assert_widget_error(
        r#"
            struct Dial;
            #[widget]
            impl Dial { fn view(&self) { () } }
        "#,
        "unsupported lifecycle hook `view`",
    );
    assert_widget_error(
        r#"
            struct Dial;
            enum First { Tick }
            enum Second { Tick }
            #[widget]
            impl Dial {
                fn children(&self) -> rawr_iced::widget::children::List<First> { panic!("children") }
                fn update(&mut self, input: rawr_iced::widget::event::Input) -> rawr_iced::widget::event::Outcome<Second> { panic!("update") }
            }
        "#,
        "constrains Message",
    );
}

#[test]
fn widget_marker_and_hook_signatures_are_closed() {
    assert_widget_error("#[widget] struct Dial;", "may mark only an inherent impl");
    assert_widget_error(
        r#"
            struct Dial;
            #[widget]
            impl Dial { fn size(&mut self) -> rawr_iced::widget::layout::Size { panic!("size") } }
        "#,
        "requires `&self`",
    );
    assert_widget_error(
        r#"
            struct Dial;
            #[widget]
            impl Dial { fn state(&self) -> i64 { 0 } }
        "#,
        "must return one nominal script type",
    );
}

#[test]
fn draw_requires_exact_scoped_mutable_renderer_and_unit_output() {
    for (source, reason) in [
        (
            r#"
                struct Dial;
                #[widget]
                impl Dial {
                    fn draw(&self, input: rawr_iced::widget::draw::Input) { () }
                }
            "#,
            "expected exact mutable `&mut rawr_iced::Renderer`",
        ),
        (
            r#"
                struct Dial;
                #[widget]
                impl Dial {
                    fn draw(&self, renderer: &rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) { () }
                }
            "#,
            "exact mutable `&mut rawr_iced::Renderer`",
        ),
        (
            r#"
                struct Dial;
                #[widget]
                impl Dial {
                    fn draw(&self, input: rawr_iced::widget::draw::Input, renderer: &mut rawr_iced::Renderer) { () }
                }
            "#,
            "expected exact mutable `&mut rawr_iced::Renderer`",
        ),
        (
            r#"
                struct Dial;
                #[widget]
                impl Dial {
                    fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) -> i64 { 1 }
                }
            "#,
            "expected unit output",
        ),
    ] {
        assert_widget_error(source, reason);
    }
}

#[test]
fn invocation_scoped_renderer_is_rejected_from_storage_and_aliases() {
    for source in [
        "struct Saved { renderer: rawr_iced::Renderer }",
        "struct Saved { renderer: rawr_iced::Envelope<rawr_iced::Renderer> }",
        "enum Saved { Renderer(rawr_iced::Renderer) }",
        "pub fn save(renderer: &mut rawr_iced::Renderer) { () }",
        "pub fn make() -> rawr_iced::Renderer { panic!(\"renderer\") }",
        r#"
            struct Dial;
            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                    let saved = renderer;
                    renderer.fill();
                }
            }
        "#,
        r#"
            use rawr_iced::save;
            struct Dial;
            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                    save(renderer);
                }
            }
        "#,
        r#"
            use rawr_iced::renderer;
            struct Dial;
            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                    renderer();
                }
            }
        "#,
    ] {
        let error = compiler().compile_executable(source).unwrap_err();
        assert!(
            matches!(
                error.err_type(),
                ParseErrorType::Resolution(resolve::Error::InvalidHostOperation { reason, .. })
                    if reason.contains("invocation-scoped")
            ),
            "unexpected error: {error:?}"
        );
    }
}

#[test]
fn lifecycle_carriers_reject_unowned_lookalikes() {
    let mut engine = Engine::new();
    let iced = host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap().finish().unwrap();
    let rawr = host::facade::Builder::new("rawr_iced", host::facade::Abi::new(1).unwrap()).unwrap().finish().unwrap();
    engine.register_host_facade(iced).unwrap();
    engine.register_host_facade(rawr).unwrap();
    engine.register_type_with_name::<Size>("rawr_iced::widget::layout::Size");
    let source = r#"
        struct Dial;
        #[widget]
        impl Dial {
            fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
        }
    "#;
    let error = engine.compile_executable(source).unwrap_err();

    assert!(
        matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidWidget { reason, .. })
                if reason.contains("legacy or unowned lookalikes")
        ),
        "unexpected error: {error:?}",
    );
}

#[test]
fn facade_abi_drift_changes_the_retained_protocol_proof() {
    let source = r#"
        struct Dial;
        #[widget]
        impl Dial {
            fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
        }
    "#;
    let first = compiler_with_abi(1).compile_executable(source).unwrap();
    let second = compiler_with_abi(2).compile_executable(source).unwrap();
    let first = &first.widgets()[0];
    let second = &second.widgets()[0];

    assert_eq!(first.dependency(), second.dependency());
    assert_ne!(first.lifecycle(), second.lifecycle());
    assert_ne!(first.fingerprint(), second.fingerprint());
}

#[test]
fn exact_widget_boundaries_select_generic_and_exact_promotions() {
    let generic = compiler()
        .compile_executable(
            r#"
                struct Dial;
                enum Message { Tick }
                #[widget]
                impl Dial { fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") } }
                pub fn view() -> iced::Element<Message> { Dial {} }
            "#,
        )
        .unwrap();
    let exact = compiler()
        .compile_executable(
            r#"
                struct Dial;
                enum Message { Tick }
                #[widget]
                impl Dial {
                    fn children(&self) -> rawr_iced::widget::children::List<Message> { panic!("children") }
                }
                pub fn view() -> iced::Element<Message> { Dial {} }
            "#,
        )
        .unwrap();

    for executable in [&generic, &exact] {
        let [promotion] = executable.widget_promotions() else {
            panic!("expected one widget promotion");
        };
        assert_eq!(promotion.message().display_name(), "main::Message");
        assert_eq!(promotion.definition().owner(), executable.widgets()[0].owner());
        assert_eq!(promotion.site().path(), [0]);
    }
}

#[test]
fn exact_widget_message_mismatch_is_a_positioned_compile_error() {
    assert_widget_error(
        r#"
            struct Dial;
            enum First { Tick }
            enum Second { Tick }
            #[widget]
            impl Dial {
                fn children(&self) -> rawr_iced::widget::children::List<First> { panic!("children") }
            }
            pub fn view() -> iced::Element<Second> { Dial {} }
        "#,
        "does not match boundary Message",
    );
}

#[test]
fn structural_widget_sites_ignore_formatting_and_distinguish_children() {
    let compact = compiler()
        .compile_executable(
            r#"
                struct Dial;
                enum Message { Tick }
                #[widget]
                impl Dial { fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") } }
                pub fn wrap(value: iced::Element<Message>) -> iced::Element<Message> { value }
                pub fn view() -> iced::Element<Message> { wrap(Dial {}) }
                pub fn other() -> iced::Element<Message> { Dial {} }
            "#,
        )
        .unwrap();
    let formatted = compiler()
        .compile_executable(
            r#"
                struct Dial;
                enum Message { Tick }

                #[widget]
                impl Dial {
                    fn size(&self) -> rawr_iced::widget::layout::Size {
                        panic!("size")
                    }
                }

                pub fn wrap(
                    value: iced::Element<Message>,
                ) -> iced::Element<Message> {
                    value
                }

                pub fn view() -> iced::Element<Message> {
                    wrap(
                        Dial {},
                    )
                }

                pub fn other() -> iced::Element<Message> {
                    Dial {}
                }
            "#,
        )
        .unwrap();
    let compact_sites = compact.widget_promotions().iter().map(|promotion| promotion.site().clone()).collect::<Vec<_>>();
    let formatted_sites = formatted.widget_promotions().iter().map(|promotion| promotion.site().clone()).collect::<Vec<_>>();

    assert_eq!(compact_sites, formatted_sites);
    assert_eq!(compact_sites.len(), 2);
    assert_ne!(compact_sites[0], compact_sites[1]);
    assert!(compact_sites.iter().any(|site| site.path().len() == 2));
}

fn compile_fingerprint(source: &str) -> widget::Fingerprint {
    compiler().compile_executable(source).unwrap().widgets()[0].fingerprint().clone()
}

fn assert_widget_error(source: &str, reason: &str) {
    let error = compiler().compile_executable(source).unwrap_err();
    assert!(
        matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidWidget { reason: actual, .. })
                if actual.contains(reason)
        ),
        "unexpected error for `{reason}`: {error:?}"
    );
    let report = Report::from_parse(Source::new("main.rs", source).unwrap(), &error).unwrap();
    assert_eq!(report.code(), code::RESOLVE_WIDGET);
}

fn compiler() -> Engine {
    compiler_with_abi(1)
}

fn compiler_with_abi(abi: u32) -> Engine {
    let mut engine = Engine::new();
    for facade in widget_facades(abi) {
        engine.register_host_facade(facade).unwrap();
    }
    engine
}

fn widget_facades(abi: u32) -> [host::facade::Definition; 2] {
    let mut iced = host::facade::Builder::new("iced", host::facade::Abi::new(1).unwrap()).unwrap();
    iced.module("mouse").unwrap();
    iced.type_::<MouseInteraction>("mouse::Interaction", &[], &[]).unwrap();
    iced.type_::<Value>("Element", &["Message"], &[]).unwrap();
    let iced = iced.finish().unwrap();

    let mut rawr = host::facade::Builder::new("rawr_iced", host::facade::Abi::new(abi).unwrap()).unwrap();
    for module in ["widget", "widget::children", "widget::layout", "widget::event", "widget::draw", "widget::mouse", "widget::operation", "widget::overlay"] {
        rawr.module(module).unwrap();
    }
    rawr.type_::<Children>("widget::children::List", &["Message"], &[]).unwrap();
    rawr.type_::<Size>("widget::layout::Size", &[], &[]).unwrap();
    rawr.invocation_type::<Renderer>("Renderer", &[]).unwrap();
    rawr.type_::<Envelope>("Envelope", &["T"], &[]).unwrap();
    rawr.type_::<LayoutInput>("widget::layout::Input", &[], &[]).unwrap();
    rawr.type_::<LayoutNode>("widget::layout::Node", &[], &[]).unwrap();
    rawr.type_::<EventInput>("widget::event::Input", &[], &[]).unwrap();
    rawr.type_::<EventOutcome>("widget::event::Outcome", &["Message"], &[]).unwrap();
    rawr.type_::<DrawInput>("widget::draw::Input", &[], &[]).unwrap();
    rawr.type_::<MouseInput>("widget::mouse::Input", &[], &[]).unwrap();
    rawr.type_::<OperationInput>("widget::operation::Input", &[], &[]).unwrap();
    rawr.type_::<OperationOutcome>("widget::operation::Outcome", &[], &[]).unwrap();
    rawr.type_::<OverlayInput>("widget::overlay::Input", &[], &[]).unwrap();
    rawr.type_::<OverlayValue>("widget::overlay::Value", &["Message"], &[]).unwrap();

    let mut children = host::signature::Builder::new();
    children.type_parameter("Message", &[]).unwrap();
    children.parameter(host::type_ref::Type::external(iced.export_type("Element").unwrap(), [host::type_ref::Type::parameter("Message").unwrap()]).unwrap());
    children.output(host::type_ref::Type::applied("widget::children::List", [host::type_ref::Type::parameter("Message").unwrap()]).unwrap());
    rawr.item(host::item::Definition::function("widget::children::one", children.finish()).unwrap()).unwrap();
    let mut save = host::signature::Builder::new();
    save.parameter(host::type_ref::Type::registered("Renderer").unwrap());
    rawr.item(host::item::Definition::function("save", save.finish()).unwrap()).unwrap();
    let mut renderer = host::signature::Builder::new();
    renderer.output(host::type_ref::Type::registered("Renderer").unwrap());
    rawr.item(host::item::Definition::function("renderer", renderer.finish()).unwrap()).unwrap();
    rawr.item(host::item::Definition::method("Renderer::fill", rawr::receiver::Mode::Mutable, host::signature::Builder::new().finish()).unwrap())
        .unwrap();
    [iced, rawr.finish().unwrap()]
}

struct Children;
#[derive(Clone)]
struct Size;
struct LayoutInput;
struct LayoutNode;
struct EventInput;
struct EventOutcome;
struct DrawInput;
struct Renderer;
struct Envelope;
struct MouseInput;
struct MouseInteraction;
struct OperationInput;
struct OperationOutcome;
struct OverlayInput;
struct OverlayValue;
struct Value;
