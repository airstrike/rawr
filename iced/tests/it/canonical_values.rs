use std::sync::Arc;

use rawr::{host, Dynamic, Engine, Shared};

#[test]
fn owned_iced_values_round_trip_under_canonical_names() {
    let size = evaluate("use iced::Size;", "Size", "Size::new(40.0, 24.0)")
        .try_cast_result::<iced::Size>()
        .unwrap();
    assert_eq!(size, iced::Size::new(40.0, 24.0));

    let fixed = evaluate(
        "use rawr_iced::widget::layout::Size;",
        "Size",
        "Size::fixed(40, 24)",
    )
    .try_cast_result::<iced::Size<iced::Length>>()
    .unwrap();
    assert_eq!(
        fixed,
        iced::Size::new(iced::Length::Fixed(40.0), iced::Length::Fixed(24.0))
    );

    let color = evaluate(
        "use iced::Color;",
        "Color",
        "Color::from_rgba(0.25, 0.5, 0.75, 1.0)",
    )
    .try_cast_result::<iced::Color>()
    .unwrap();
    assert_eq!(color, iced::Color::from_rgba(0.25, 0.5, 0.75, 1.0));

    let padding = evaluate("use iced::Padding;", "Padding", "Padding::new(12)")
        .try_cast_result::<iced::Padding>()
        .unwrap();
    assert_eq!(padding, iced::Padding::new(12.0));

    let point = evaluate("use iced::Point;", "Point", "Point::new(-2.5, 4.0)")
        .try_cast_result::<iced::Point>()
        .unwrap();
    assert_eq!(point, iced::Point::new(-2.5, 4.0));

    let rectangle = evaluate(
        "use iced::{Point, Rectangle, Size};",
        "Rectangle",
        "Rectangle::new(Point::new(1.0, 2.0), Size::new(40.0, 24.0))",
    )
    .try_cast_result::<iced::Rectangle>()
    .unwrap();
    assert_eq!(
        rectangle,
        iced::Rectangle::new(iced::Point::new(1.0, 2.0), iced::Size::new(40.0, 24.0))
    );

    let vector = evaluate("use iced::Vector;", "Vector", "Vector::new(-2.5, 4.0)")
        .try_cast_result::<iced::Vector>()
        .unwrap();
    assert_eq!(vector, iced::Vector::new(-2.5, 4.0));
}

#[test]
fn every_exposed_mouse_interaction_keeps_iced_semantics() {
    use iced::mouse::Interaction;

    for (name, expected) in [
        ("None", Interaction::None),
        ("Hidden", Interaction::Hidden),
        ("Idle", Interaction::Idle),
        ("ContextMenu", Interaction::ContextMenu),
        ("Help", Interaction::Help),
        ("Pointer", Interaction::Pointer),
        ("Progress", Interaction::Progress),
        ("Wait", Interaction::Wait),
        ("Cell", Interaction::Cell),
        ("Crosshair", Interaction::Crosshair),
        ("Text", Interaction::Text),
        ("Alias", Interaction::Alias),
        ("Copy", Interaction::Copy),
        ("Move", Interaction::Move),
        ("NoDrop", Interaction::NoDrop),
        ("NotAllowed", Interaction::NotAllowed),
        ("Grab", Interaction::Grab),
        ("Grabbing", Interaction::Grabbing),
        ("ResizingHorizontally", Interaction::ResizingHorizontally),
        ("ResizingVertically", Interaction::ResizingVertically),
        ("ResizingDiagonallyUp", Interaction::ResizingDiagonallyUp),
        (
            "ResizingDiagonallyDown",
            Interaction::ResizingDiagonallyDown,
        ),
        ("ResizingColumn", Interaction::ResizingColumn),
        ("ResizingRow", Interaction::ResizingRow),
        ("AllScroll", Interaction::AllScroll),
        ("ZoomIn", Interaction::ZoomIn),
        ("ZoomOut", Interaction::ZoomOut),
    ] {
        let value = evaluate(
            "use iced::mouse::Interaction;",
            "Interaction",
            &format!("Interaction::{name}"),
        )
        .try_cast_result::<Interaction>()
        .unwrap();
        assert_eq!(value, expected, "interaction `{name}` drifted");
    }
}

#[test]
fn invalid_canonical_values_fail_instead_of_being_laundered() {
    assert!(invoke(
        "use iced::Color;",
        "Color",
        "Color::from_rgb(1.1, 0.0, 0.0)"
    )
    .is_err());
    assert!(invoke(
        "use iced::{Point, Rectangle, Size};",
        "Rectangle",
        "Rectangle::new(Point::new(0.0, 0.0), Size::new(-1.0, 1.0))",
    )
    .is_err());
    let error = invoke("use iced::Length;", "Length", "Length::FillPortion(0)").unwrap_err();
    assert!(
        error.contains("Length::FillPortion expects a positive value, found 0"),
        "unexpected runtime diagnostic: {error}"
    );
}

fn evaluate(imports: &str, output: &str, body: &str) -> Dynamic {
    invoke(imports, output, body).unwrap()
}

fn invoke(imports: &str, output: &str, body: &str) -> Result<Dynamic, String> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    let engine = Shared::new(engine);
    let candidate = engine
        .compile_application(application(imports, output, body))
        .map_err(|error| error.to_string())?;
    let items = candidate.items().cloned().collect::<Vec<_>>();
    let descriptor = candidate.validate().unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    let launch = Arc::new(launch.bind(registry).unwrap());
    let item = items
        .iter()
        .find(|item| item.key().name() == "make")
        .unwrap();
    launch
        .invoke(item, &mut [])
        .map_err(|error| error.to_string())
}

fn application(imports: &str, output: &str, body: &str) -> String {
    format!(
        r#"
        {imports}
        use iced::widget::{{Text, text}};

        #[derive(Default)]
        struct State;

        #[derive(Debug, Clone, Copy)]
        enum Message {{ Tick }}

        pub fn make() -> {output} {{ {body} }}

        impl State {{
            fn update(&mut self, _message: Message) {{}}
            fn view(&self) -> Text {{ text("canonical values") }}
        }}

        pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
        "#
    )
}
