use rawr::{capability, host, receiver, resolve, Engine, ParseErrorType};

#[test]
fn iced_facade_is_deterministic_and_declares_static_contracts() {
    let first = rawr_iced::facade().unwrap();
    let second = rawr_iced::facade().unwrap();
    let extension = rawr_iced::extension(&first).unwrap();
    let second_extension = rawr_iced::extension(&second).unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.abi().get(), 16);
    assert_eq!(extension.fingerprint(), second_extension.fingerprint());
    assert_eq!(extension.abi().get(), 4);

    let task = first.types().find(|ty| ty.path() == "iced::Task").unwrap();
    assert_eq!(task.usage(), host::usage::Policy::Required);
    let none = first
        .items()
        .find(|item| item.path() == "iced::Task::none")
        .unwrap();
    assert_eq!(none.outcome(), host::usage::Outcome::Discardable);
    assert_eq!(none.evaluation(), host::usage::Evaluation::Elidable);
    assert_eq!(
        extension
            .items()
            .find(|item| item.path() == "rawr_iced::task::batch")
            .unwrap()
            .kind(),
        host::item::Kind::IntrinsicMacro
    );
    for path in ["iced::Task::chain", "iced::Task::map"] {
        let item = first.items().find(|item| item.path() == path).unwrap();
        assert_eq!(item.kind(), host::item::Kind::Method);
        assert_eq!(item.receiver(), Some(receiver::Mode::Owned));
    }
    let perform = first
        .items()
        .find(|item| item.path() == "iced::Task::perform")
        .unwrap();
    assert_eq!(perform.kind(), host::item::Kind::AssociatedFunction);
    assert_eq!(perform.signature().parameters().len(), 2);

    let subscription = first
        .types()
        .find(|ty| ty.path() == "iced::Subscription")
        .unwrap();
    assert_eq!(subscription.usage(), host::usage::Policy::Required);
    let none = first
        .items()
        .find(|item| item.path() == "iced::Subscription::none")
        .unwrap();
    assert_eq!(none.outcome(), host::usage::Outcome::Discardable);
    assert_eq!(none.evaluation(), host::usage::Evaluation::Elidable);
    assert_eq!(
        extension
            .items()
            .find(|item| item.path() == "rawr_iced::subscription::batch")
            .unwrap()
            .kind(),
        host::item::Kind::IntrinsicMacro
    );
    let map = first
        .items()
        .find(|item| item.path() == "iced::Subscription::map")
        .unwrap();
    assert_eq!(map.kind(), host::item::Kind::Method);
    assert_eq!(map.receiver(), Some(receiver::Mode::Owned));

    let renderer = extension
        .types()
        .find(|ty| ty.path() == "rawr_iced::Renderer")
        .unwrap();
    assert_eq!(renderer.storage(), host::type_def::Storage::Invocation);
    assert_eq!(renderer.capabilities(), capability::Set::new());
    assert!(renderer.projections().is_empty());
    let renderer_output = host::type_ref::Type::registered("rawr_iced::Renderer").unwrap();
    assert!(extension
        .items()
        .all(|item| item.signature().output() != &renderer_output));
    assert_eq!(
        extension
            .items()
            .filter(|item| item.path().starts_with("rawr_iced::Renderer::"))
            .map(|item| item.path())
            .collect::<Vec<_>>(),
        [
            "rawr_iced::Renderer::fill_quad",
            "rawr_iced::Renderer::fill_rounded_quad"
        ]
    );

    let mut registry = host::Registry::new();
    registry.install(first).unwrap();
    registry.install(extension).unwrap();
    assert!(registry.type_("iced::Result").is_some());
    assert!(registry.type_("iced::Task").is_some());
    assert!(registry.type_("iced::Subscription").is_some());
    assert!(registry.type_("iced::Theme").is_some());
    assert!(registry.type_("iced::Element").is_some());
    assert!(registry.type_("iced::widget::Value").is_none());
    for path in [
        "iced::Color",
        "iced::Length",
        "iced::Padding",
        "iced::Point",
        "iced::Rectangle",
        "iced::Size",
        "iced::Vector",
        "iced::mouse::Interaction",
    ] {
        assert!(registry.type_(path).is_some(), "missing canonical `{path}`");
    }
    for path in [
        "iced::widget::button::Builtin",
        "iced::widget::text::Builtin",
        "iced::widget::container::Builtin",
    ] {
        let style = second.types().find(|ty| ty.path() == path).unwrap();
        assert_eq!(style.visibility(), host::type_def::Visibility::Private);
        assert!(registry.type_(path).is_some(), "missing private `{path}`");
    }
    for path in [
        "iced::widget::Size",
        "iced::Renderer",
        "iced::theme::Token",
        "iced::widget::children::List",
        "iced::widget::layout::Input",
        "iced::widget::draw::List",
        "iced::widget::draw::Scene",
        "iced::widget::draw::Frame",
        "iced::widget::mouse::Interaction",
    ] {
        assert!(registry.type_(path).is_none(), "retained removed `{path}`");
    }
    for path in [
        "rawr_iced::Renderer",
        "rawr_iced::theme::Palette",
        "rawr_iced::widget::children::List",
        "rawr_iced::widget::layout::Input",
        "rawr_iced::widget::layout::Size",
    ] {
        assert!(registry.type_(path).is_some(), "missing extension `{path}`");
    }
    let size = registry.item("iced::Size::new").unwrap();
    assert_eq!(
        size.signature().parameters(),
        &[
            host::type_ref::Type::builtin(host::type_ref::Builtin::F64),
            host::type_ref::Type::builtin(host::type_ref::Builtin::F64),
        ]
    );
    assert_eq!(
        size.signature().output(),
        &host::type_ref::Type::registered("iced::Size").unwrap()
    );
    let rectangle = registry.item("iced::Rectangle::new").unwrap();
    assert_eq!(
        rectangle.signature().parameters(),
        &[
            host::type_ref::Type::registered("iced::Point").unwrap(),
            host::type_ref::Type::registered("iced::Size").unwrap(),
        ]
    );
    for (path, inputs) in [
        (
            "iced::Length::Fixed",
            &[host::type_ref::Builtin::F64, host::type_ref::Builtin::I64][..],
        ),
        (
            "iced::Length::FillPortion",
            &[host::type_ref::Builtin::I64][..],
        ),
    ] {
        let overloads = second
            .items()
            .filter(|item| item.path() == path)
            .map(|item| item.signature().parameters().to_vec())
            .collect::<Vec<_>>();
        let expected = inputs
            .iter()
            .map(|input| vec![host::type_ref::Type::builtin(*input)])
            .collect::<Vec<_>>();
        assert_eq!(overloads, expected);
    }
    let layout = second_extension
        .items()
        .find(|item| item.path() == "rawr_iced::widget::layout::Size::new")
        .unwrap();
    let length = host::type_ref::Type::external(second.export_type("Length").unwrap(), []).unwrap();
    assert_eq!(layout.signature().parameters(), &[length.clone(), length]);
    assert_eq!(
        layout.signature().output(),
        &host::type_ref::Type::registered("rawr_iced::widget::layout::Size").unwrap()
    );
    for path in [
        "iced::widget::Column",
        "iced::widget::Row",
        "iced::widget::Container",
        "iced::widget::Button",
        "iced::widget::TextInput",
        "iced::widget::Checkbox",
        "iced::widget::Toggler",
        "iced::widget::Scrollable",
    ] {
        let widget = registry.type_(path).unwrap();
        assert_eq!(widget.projections().len(), 1);
        assert_eq!(widget.projections()[0].target(), "iced::Element");
        assert_eq!(
            widget.projections()[0].arguments(),
            &[host::type_def::Argument::source("Message").unwrap()]
        );
    }
    for path in ["iced::widget::Text", "iced::widget::Space"] {
        let widget = registry.type_(path).unwrap();
        assert!(widget.parameters().is_empty());
        assert_eq!(widget.projections().len(), 1);
        assert_eq!(widget.projections()[0].target(), "iced::Element");
        assert_eq!(
            widget.projections()[0].arguments(),
            &[host::type_def::Argument::fresh()]
        );
    }
    let slider_type = registry.type_("iced::widget::Slider").unwrap();
    assert_eq!(slider_type.parameters(), ["T", "Message"]);
    assert_eq!(slider_type.projections()[0].target(), "iced::Element");
    assert_eq!(
        slider_type.projections()[0].arguments(),
        &[host::type_def::Argument::source("Message").unwrap()]
    );
    assert_eq!(
        registry.item("iced::run").unwrap().kind(),
        host::item::Kind::IntrinsicFunction
    );
    // `column![..]` and `column(..)`: a macro and a function of one name.
    for path in ["iced::widget::column", "iced::widget::row"] {
        let mut kinds = second
            .items()
            .filter(|item| item.path() == path)
            .map(|item| item.kind())
            .collect::<Vec<_>>();
        kinds.sort();
        assert_eq!(
            kinds,
            [host::item::Kind::Function, host::item::Kind::IntrinsicMacro]
        );
    }
    assert_eq!(
        registry.item("iced::Application::run").unwrap().kind(),
        host::item::Kind::IntrinsicMethod
    );
    assert_eq!(
        registry.item("iced::Application::run").unwrap().receiver(),
        Some(receiver::Mode::Owned)
    );
    assert_eq!(
        registry
            .item("iced::Application::subscription")
            .unwrap()
            .receiver(),
        Some(receiver::Mode::Owned)
    );
    for path in ["iced::Application::title", "iced::Application::centered"] {
        assert_eq!(
            registry.item(path).unwrap().receiver(),
            Some(receiver::Mode::Owned)
        );
    }
    assert_eq!(
        registry.item("iced::Center").unwrap().kind(),
        host::item::Kind::Value
    );
    assert_eq!(
        registry
            .item("iced::widget::Button::on_press")
            .unwrap()
            .receiver(),
        Some(receiver::Mode::Owned)
    );
    for (path, style) in [
        (
            "iced::widget::Button::style",
            "iced::widget::button::Builtin",
        ),
        ("iced::widget::Text::style", "iced::widget::text::Builtin"),
        (
            "iced::widget::Container::style",
            "iced::widget::container::Builtin",
        ),
    ] {
        let item = second
            .items()
            .find(|item| {
                item.path() == path
                    && item.signature().parameters()
                        == [host::type_ref::Type::registered(style).unwrap()]
            })
            .unwrap();
        assert_eq!(item.kind(), host::item::Kind::Method);
        assert_eq!(item.receiver(), Some(receiver::Mode::Owned));
    }
    for (module, names, style) in [
        (
            "button",
            &[
                "primary",
                "secondary",
                "success",
                "warning",
                "danger",
                "text",
                "background",
                "subtle",
            ][..],
            "iced::widget::button::Builtin",
        ),
        (
            "text",
            &[
                "base",
                "primary",
                "secondary",
                "success",
                "warning",
                "danger",
            ][..],
            "iced::widget::text::Builtin",
        ),
        (
            "container",
            &[
                "transparent",
                "rounded_box",
                "bordered_box",
                "dark",
                "primary",
                "secondary",
                "success",
                "warning",
                "danger",
            ][..],
            "iced::widget::container::Builtin",
        ),
    ] {
        for name in names {
            let path = format!("iced::widget::{module}::{name}");
            // A button style is also callable, as iced's `fn` items are.
            let item = second
                .items()
                .find(|item| item.path() == path && item.kind() == host::item::Kind::Value)
                .unwrap();
            assert_eq!(
                item.signature().output(),
                &host::type_ref::Type::registered(style).unwrap()
            );
        }
    }
    let keys = second_extension
        .items()
        .filter(|item| item.path() == "rawr_iced::widget::key")
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 2);
    assert!(keys
        .iter()
        .all(|item| { item.kind() == host::item::Kind::Function && item.receiver().is_none() }));
    let inputs = keys
        .iter()
        .map(|item| item.signature().parameters()[1].clone())
        .collect::<Vec<_>>();
    assert!(inputs.contains(&host::type_ref::Type::builtin(host::type_ref::Builtin::I64)));
    assert!(inputs.contains(&host::type_ref::Type::builtin(
        host::type_ref::Builtin::String
    )));
    // `impl Into<Pixels>`: one owned overload per accepted input.
    assert_eq!(
        inputs_of(&second, "iced::widget::Text::size"),
        [host::type_ref::Builtin::F64, host::type_ref::Builtin::I64]
            .map(host::type_ref::Type::builtin)
    );
    // `impl Into<Element>`: a label, or any element.
    assert_eq!(
        inputs_of(&second, "iced::widget::button"),
        [
            host::type_ref::Type::builtin(host::type_ref::Builtin::String),
            host::type_ref::Type::applied(
                "iced::Element",
                [host::type_ref::Type::parameter("Message").unwrap()]
            )
            .unwrap(),
        ]
    );
    let mut text = second
        .items()
        .filter(|item| {
            item.path() == "iced::widget::text" && item.kind() == host::item::Kind::Function
        })
        .map(|item| item.signature().parameters()[0].clone())
        .collect::<Vec<_>>();
    text.sort();
    let mut expected = [
        host::type_ref::Builtin::String,
        host::type_ref::Builtin::I64,
        host::type_ref::Builtin::Bool,
        host::type_ref::Builtin::Char,
        host::type_ref::Builtin::F64,
    ]
    .map(host::type_ref::Type::builtin)
    .to_vec();
    expected.sort();
    assert_eq!(text, expected);
    for (path, input) in [
        (
            "iced::widget::TextInput::on_input",
            host::type_ref::Builtin::String,
        ),
        (
            "iced::widget::Checkbox::on_toggle",
            host::type_ref::Builtin::Bool,
        ),
        (
            "iced::widget::Toggler::on_toggle",
            host::type_ref::Builtin::Bool,
        ),
    ] {
        let item = registry.item(path).unwrap();
        assert_eq!(item.receiver(), Some(receiver::Mode::Owned));
        assert_eq!(
            item.signature().parameters(),
            &[host::type_ref::Type::callable(
                host::type_ref::Type::builtin(input),
                host::type_ref::Type::parameter("Message").unwrap(),
            )]
        );
    }
    for path in [
        "iced::widget::TextInput::secure",
        "iced::widget::Checkbox::label",
        "iced::widget::Toggler::label",
        "iced::widget::Slider::step",
    ] {
        assert_eq!(
            registry.item(path).unwrap().receiver(),
            Some(receiver::Mode::Owned)
        );
    }
    let paddings = second
        .items()
        .filter(|item| item.path() == "iced::widget::TextInput::padding")
        .collect::<Vec<_>>();
    assert_eq!(paddings.len(), 5);
    assert!(paddings.iter().all(|item| {
        item.kind() == host::item::Kind::Method && item.receiver() == Some(receiver::Mode::Owned)
    }));
    let padding_inputs = paddings
        .iter()
        .map(|item| item.signature().parameters()[0].clone())
        .collect::<Vec<_>>();
    assert!(padding_inputs.contains(&host::type_ref::Type::builtin(host::type_ref::Builtin::I64)));
    assert!(padding_inputs.contains(&host::type_ref::Type::builtin(host::type_ref::Builtin::F64)));
    assert!(padding_inputs.contains(&host::type_ref::Type::registered("iced::Padding").unwrap()));
    // `slider` over `i64`, over `f64` as iced's `f32` sliders, and over
    // `Degrees` and `Radians`.
    let sliders = second
        .items()
        .filter(|item| item.path() == "iced::widget::slider")
        .collect::<Vec<_>>();
    assert_eq!(sliders.len(), 4);
    for (slider, range, number) in [
        (
            sliders[0],
            host::type_ref::Builtin::InclusiveRange,
            host::type_ref::Builtin::I64,
        ),
        (
            sliders[1],
            host::type_ref::Builtin::FloatInclusiveRange,
            host::type_ref::Builtin::F64,
        ),
    ] {
        assert_eq!(slider.kind(), host::item::Kind::Function);
        assert_eq!(
            slider.signature().parameters(),
            &[
                host::type_ref::Type::builtin(range),
                host::type_ref::Type::builtin(number),
                host::type_ref::Type::callable(
                    host::type_ref::Type::builtin(number),
                    host::type_ref::Type::parameter("Message").unwrap(),
                ),
            ]
        );
        assert_eq!(
            slider.signature().output(),
            &host::type_ref::Type::applied(
                "iced::widget::Slider",
                [
                    host::type_ref::Type::builtin(number),
                    host::type_ref::Type::parameter("Message").unwrap(),
                ],
            )
            .unwrap()
        );
    }
    // `Alignment`, or `alignment::Horizontal`.
    let align_x = second
        .items()
        .filter(|item| item.path() == "iced::widget::Column::align_x")
        .collect::<Vec<_>>();
    assert_eq!(align_x.len(), 2);
    assert!(align_x
        .iter()
        .all(|item| item.receiver() == Some(receiver::Mode::Owned)));
    for path in ["iced::widget::scrollable", "iced::widget::space"] {
        assert!(registry.item(path).is_some(), "missing shell item `{path}`");
    }
    // `Text::align_x` takes `Alignment` or `alignment::Horizontal` too.
    assert_eq!(
        second
            .items()
            .filter(|item| item.path() == "iced::widget::Text::align_x")
            .count(),
        2
    );
    // `impl Into<Length>` and `impl Into<Padding>`: the value, or pixels.
    for path in [
        "iced::widget::Text::width",
        "iced::widget::Column::width",
        "iced::widget::Container::center_x",
        "iced::widget::Container::center_y",
        "iced::widget::Space::width",
        "iced::widget::Space::height",
    ] {
        assert_eq!(inputs_of(&second, path).len(), 3, "`{path}` overloads");
    }
    // Padding also takes `[vertical, horizontal]` in pixels.
    for path in [
        "iced::widget::Button::padding",
        "iced::widget::Column::padding",
    ] {
        assert_eq!(inputs_of(&second, path).len(), 5, "`{path}` overloads");
    }

    let message = registry
        .item("iced::run")
        .unwrap()
        .signature()
        .requirements("Message")
        .unwrap();
    assert!(message.contains(capability::Kind::Clone));
    assert!(message.contains(capability::Kind::Send));
    assert!(message.contains(capability::Kind::Static));
}

#[test]
fn iced_and_widget_use_paths_resolve_without_runtime_modules() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    let source = r#"
use iced::Center;
use iced::{Element, Task, Theme};
use iced::widget::{Checkbox, Column, Text, TextInput, Toggler, button, checkbox, column, text, text_input, toggler};

#[derive(Debug, Clone, Copy)]
enum Message { Increment, Decrement }

struct Surface { content: Column<'_, Message> }
struct Contracts { task: Task<Message>, theme: Theme, element: Element<Message>, text: Text }
struct Controls { input: TextInput<Message>, checkbox: Checkbox<Message>, toggler: Toggler<Message> }
"#;
    let executable = engine.compile_executable(source).unwrap();
    assert_eq!(executable.type_count(), 4);

    let error = engine
        .compile("use iced::widget::{Column, missing};")
        .unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownImport { path, .. })
            if path == "iced::widget::missing"
    ));

    let error = engine.compile("use iced::widget::Value;").unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownImport { path, .. })
            if path == "iced::widget::Value"
    ));

    // iced's `Application` builder and `Program` trait are nameable, as
    // iced master exports them.
    engine
        .compile("use iced::{Application, Program};")
        .unwrap_or_else(|error| panic!("{error}"));

    for path in [
        "iced::widget::button::Builtin",
        "iced::widget::text::Builtin",
        "iced::widget::container::Builtin",
    ] {
        let error = engine.compile(format!("use {path};")).unwrap_err();
        assert!(matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::UnknownImport { path: actual, .. })
                if actual == path
        ));
    }

    assert!(engine.eval::<rawr::Dynamic>("iced::run((), ())").is_err());
}

#[test]
fn rawr_extension_inventory_is_separate_and_external_contract_is_exact() {
    let iced = rawr_iced::facade().unwrap();
    let rawr = rawr_iced::extension(&iced).unwrap();

    for path in [
        "rawr_iced::Renderer",
        "rawr_iced::theme::Palette",
        "rawr_iced::widget::children::List",
        "rawr_iced::widget::layout::Size",
        "rawr_iced::widget::layout::Input",
        "rawr_iced::widget::layout::Node",
        "rawr_iced::widget::event::Input",
        "rawr_iced::widget::event::Outcome",
        "rawr_iced::widget::draw::Input",
        "rawr_iced::widget::mouse::Input",
        "rawr_iced::widget::operation::Input",
        "rawr_iced::widget::operation::Outcome",
        "rawr_iced::widget::overlay::Input",
        "rawr_iced::widget::overlay::Value",
    ] {
        assert!(rawr.types().any(|ty| ty.path() == path), "missing `{path}`");
        // iced's own `theme::Palette` is the theme's palette, read by style
        // functions; rawr_iced's is the seed of a custom theme.
        if path == "rawr_iced::theme::Palette" {
            continue;
        }
        assert!(iced
            .types()
            .all(|ty| ty.path() != path.replacen("rawr_iced", "iced", 1)));
    }
    for path in [
        "rawr_iced::theme::Token",
        "rawr_iced::theme::Styles",
        "rawr_iced::theme::Button",
        "rawr_iced::theme::Text",
    ] {
        assert!(
            rawr.types().all(|ty| ty.path() != path),
            "retained `{path}`"
        );
    }
    let custom = rawr
        .items()
        .find(|item| item.path() == "rawr_iced::theme::custom")
        .unwrap();
    assert_eq!(
        custom.signature().parameters(),
        &[
            host::type_ref::Type::builtin(host::type_ref::Builtin::String),
            host::type_ref::Type::registered("rawr_iced::theme::Palette").unwrap(),
        ]
    );
    assert_eq!(
        custom.signature().output(),
        &host::type_ref::Type::external(iced.export_type("Theme").unwrap(), []).unwrap()
    );
    for path in [
        "rawr_iced::widget::role",
        "rawr_iced::theme::Primary",
        "rawr_iced::theme::Secondary",
        "rawr_iced::theme::Success",
        "rawr_iced::theme::Warning",
        "rawr_iced::theme::Danger",
        "rawr_iced::theme::Muted",
        "rawr_iced::theme::Styles::new",
        "rawr_iced::theme::Styles::button",
        "rawr_iced::theme::Styles::text",
        "rawr_iced::theme::Button::new",
        "rawr_iced::theme::Text::new",
    ] {
        assert!(
            rawr.items().all(|item| item.path() != path),
            "retained `{path}`"
        );
    }

    for (old, moved) in [
        ("iced::Task::batch", "rawr_iced::task::batch"),
        (
            "iced::Subscription::batch",
            "rawr_iced::subscription::batch",
        ),
        ("iced::Theme::custom", "rawr_iced::theme::custom"),
        (
            "iced::theme::Palette::new",
            "rawr_iced::theme::Palette::new",
        ),
        ("iced::Element::key", "rawr_iced::widget::key"),
        (
            "iced::widget::children::list",
            "rawr_iced::widget::children::list",
        ),
        (
            "iced::Size::fixed",
            "rawr_iced::widget::layout::Size::fixed",
        ),
        (
            "iced::widget::layout::Input::resolve",
            "rawr_iced::widget::layout::Input::resolve",
        ),
        (
            "iced::widget::layout::Node::child",
            "rawr_iced::widget::layout::Node::child",
        ),
        (
            "iced::widget::event::Input::is_press",
            "rawr_iced::widget::event::Input::is_press",
        ),
        (
            "iced::widget::event::ignore",
            "rawr_iced::widget::event::ignore",
        ),
        (
            "iced::widget::event::publish",
            "rawr_iced::widget::event::publish",
        ),
        (
            "iced::widget::event::Outcome::when",
            "rawr_iced::widget::event::Outcome::when",
        ),
        (
            "iced::Renderer::fill_quad",
            "rawr_iced::Renderer::fill_quad",
        ),
        (
            "iced::Renderer::fill_rounded_quad",
            "rawr_iced::Renderer::fill_rounded_quad",
        ),
        (
            "iced::widget::draw::Input::bounds",
            "rawr_iced::widget::draw::Input::bounds",
        ),
        (
            "iced::widget::draw::Input::viewport",
            "rawr_iced::widget::draw::Input::viewport",
        ),
        (
            "iced::widget::draw::Input::text_color",
            "rawr_iced::widget::draw::Input::text_color",
        ),
        (
            "iced::widget::mouse::Input::is_over",
            "rawr_iced::widget::mouse::Input::is_over",
        ),
        (
            "iced::widget::operation::Input::bounds",
            "rawr_iced::widget::operation::Input::bounds",
        ),
        (
            "iced::widget::operation::skip",
            "rawr_iced::widget::operation::skip",
        ),
        (
            "iced::widget::operation::container",
            "rawr_iced::widget::operation::container",
        ),
        (
            "iced::widget::operation::traverse",
            "rawr_iced::widget::operation::traverse",
        ),
        (
            "iced::widget::overlay::Input::viewport",
            "rawr_iced::widget::overlay::Input::viewport",
        ),
        (
            "iced::widget::overlay::Input::translation",
            "rawr_iced::widget::overlay::Input::translation",
        ),
        (
            "iced::widget::overlay::none",
            "rawr_iced::widget::overlay::none",
        ),
        (
            "iced::widget::overlay::some",
            "rawr_iced::widget::overlay::some",
        ),
    ] {
        assert!(
            rawr.items().any(|item| item.path() == moved),
            "missing `{moved}`"
        );
        // iced's own `Theme::custom(name, Seed)` and `Task::batch(list)`/
        // `Subscription::batch(list)` are in the iced facade; the hex-palette
        // and `batch![..]` forms stay Rawr's.
        if !matches!(
            old,
            "iced::Theme::custom" | "iced::Task::batch" | "iced::Subscription::batch"
        ) {
            assert!(iced.items().all(|item| item.path() != old));
        }
    }

    let mut missing = host::Registry::new();
    assert!(matches!(
        missing.install(rawr_iced::extension(&iced).unwrap()),
        Err(host::Error::MissingFacadeDependency { source, facade })
            if source.starts_with("rawr_iced::") && facade == "iced"
    ));

    let incompatible = host::facade::Builder::new("iced", host::facade::Abi::new(13).unwrap())
        .unwrap()
        .finish()
        .unwrap();
    let mut mismatched = host::Registry::new();
    mismatched.install(incompatible).unwrap();
    assert!(matches!(
        mismatched.install(rawr),
        Err(host::Error::FacadeDependencyMismatch { source, target })
            if source.starts_with("rawr_iced::") && target.starts_with("iced::")
    ));
    assert!(mismatched.type_("rawr_iced::Renderer").is_none());
}

#[test]
fn moved_iced_paths_fail_at_the_exact_source_import() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();

    for path in [
        "iced::theme::Token",
        "iced::widget::children::List",
        "iced::widget::layout::Input",
        "iced::widget::event::Outcome",
        "iced::widget::overlay::Value",
    ] {
        let source = format!("use {path};");
        let error = engine.compile(&source).unwrap_err();
        assert!(matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::UnknownImport { path: actual, .. })
                if actual == path
        ));
        assert!(!error.primary_span().is_none());
    }

    for path in [
        "rawr_iced::widget::role",
        "rawr_iced::theme::Token",
        "rawr_iced::theme::Token::new",
        "rawr_iced::theme::Styles",
        "rawr_iced::theme::Styles::new",
        "rawr_iced::theme::Button",
        "rawr_iced::theme::Text",
        "rawr_iced::theme::Primary",
        "rawr_iced::theme::Muted",
    ] {
        let source = format!("use {path};");
        let error = engine.compile(&source).unwrap_err();
        assert!(matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::UnknownImport { path: actual, .. })
                if actual == path
        ));
        assert!(!error.primary_span().is_none());
    }

    let error = engine
        .compile("use iced::Length; pub fn old() -> Length { Length::fixed(10) }")
        .unwrap_err();
    assert!(matches!(
        error.err_type(),
        ParseErrorType::Resolution(resolve::Error::UnknownMember { owner, member, .. })
            if owner == "iced::Length" && member == "fixed"
    ));
    assert!(!error.primary_span().is_none());
}

mod element_declaration {
    #[test]
    fn excludes_invocation_state() {
        let source = include_str!("../../src/widget/value.rs");
        let declaration = source
            .split_once("pub struct Value {")
            .and_then(|(_, tail)| tail.split_once("}\n"))
            .map(|(fields, _)| fields)
            .expect("the durable Element carrier declaration must remain inspectable");

        assert!(declaration.contains("Arc<dyn widget::Descriptor>"));
        for forbidden in [
            "iced::Element",
            "dyn iced::advanced::Widget",
            "renderer::Lease",
            "&'",
        ] {
            assert!(
                !declaration.contains(forbidden),
                "durable Element carrier retained forbidden native state `{forbidden}`"
            );
        }

        let facade = include_str!("../../src/lib.rs");
        assert!(facade.contains("type_::<widget::Value>(\"Element\""));
        assert!(!facade.contains("type_::<iced::Element"));

        let facade = rawr_iced::facade().unwrap();
        let element = facade
            .types()
            .find(|ty| ty.path() == "iced::Element")
            .unwrap();
        assert_eq!(
            element.native_identity(),
            Some(std::any::TypeId::of::<rawr_iced::widget::Value>())
        );
    }
}

#[test]
fn installing_iced_twice_fails_without_mutating_the_first_contract() {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let error = rawr_iced::install(&mut engine).unwrap_err();
    assert!(matches!(
        error,
        host::Error::RegistryConflict { path, .. } if path == "iced"
    ));

    engine
        .compile("use iced::Result; struct Outcome { value: Result }")
        .unwrap();
}

/// The single inputs of every owned overload at `path`.
fn inputs_of(facade: &host::facade::Definition, path: &str) -> Vec<host::type_ref::Type> {
    facade
        .items()
        .filter(|item| item.path() == path)
        .inspect(|item| {
            assert!(item
                .receiver()
                .is_none_or(|mode| mode == receiver::Mode::Owned));
        })
        .map(|item| item.signature().parameters()[0].clone())
        .collect()
}

/// The names under `iced::widget` a snippet host imports for a snippet
/// with no `use` of its own: every free function, macro and type.
fn widget_prelude_names() -> Vec<String> {
    let facade = rawr_iced::facade().unwrap();
    let direct = |path: &str| {
        path.strip_prefix("iced::widget::")
            .is_some_and(|name| !name.contains("::"))
    };
    let mut names = facade
        .items()
        .filter(|item| {
            matches!(
                item.kind(),
                host::item::Kind::Function
                    | host::item::Kind::IntrinsicFunction
                    | host::item::Kind::IntrinsicMacro
            )
        })
        .map(host::item::Definition::path)
        .chain(facade.types().map(host::type_def::Definition::path))
        .filter(|path| direct(path))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Widget names the prelude leaves out: `iced` would hide the crate, and
/// `Widget` comes from `iced::Widget`.
const NOT_IN_PRELUDE: [&str; 3] = [
    "iced::widget::iced",
    "iced::widget::Renderer",
    "iced::widget::Widget",
];

#[test]
fn the_snippet_prelude_names_every_widget_helper() {
    let manifest = rawr_iced::MANIFEST;
    let missing = widget_prelude_names()
        .into_iter()
        .filter(|path| !NOT_IN_PRELUDE.contains(&path.as_str()))
        .filter(|path| !manifest.contains(&format!("\"{path}\"")))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "add these to `prelude` in iced/manifest.toml: {missing:?}"
    );
    // Each prelude name is one the facade declares: a type, an item, a
    // module of them (`iced::border`), or the alias `iced::Renderer`.
    let facade = rawr_iced::facade().unwrap();
    let block = &manifest[manifest.find("prelude = [").unwrap()..];
    let block = &block[..block.find(']').unwrap()];
    let prelude = block.split('"').skip(1).step_by(2).collect::<Vec<_>>();
    let declares = |path: &str| {
        let module = format!("{path}::");
        facade
            .types()
            .any(|ty| ty.path() == path || ty.path().starts_with(&module))
            || facade
                .items()
                .any(|item| item.path() == path || item.path().starts_with(&module))
    };
    let unknown = prelude
        .iter()
        .filter(|path| **path != "iced::Renderer" && !declares(path))
        .collect::<Vec<_>>();
    assert!(unknown.is_empty(), "the facade declares no {unknown:?}");
}
