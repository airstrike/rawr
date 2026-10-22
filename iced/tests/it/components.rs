use rawr::{host, Engine, Shared};
use rawr_iced::{program, runtime};

#[test]
fn ordinary_component_maps_its_local_message_into_the_parent() {
    let mut adapter = boot(&component_source("Message::Child"));

    assert_text(&adapter, "0");
    let increment = click(&adapter, "Increment child");
    assert_eq!(increment.instance(), adapter.instance());
    assert_eq!(increment.generation(), adapter.generation());
    assert_eq!(
        adapter.update(increment).unwrap(),
        program::Delivery::Applied
    );
    assert_text(&adapter, "1");
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn tabbed_root_selects_seven_mapped_child_values() {
    let mut adapter = boot(tabbed_tour_source());

    for (tab, page, action, count) in [
        ("Start tab", "Start page", "Start action", "1"),
        (
            "Text input tab",
            "Text input page",
            "Text input action",
            "2",
        ),
        ("Choices tab", "Choices page", "Choices action", "3"),
        ("Slider tab", "Slider page", "Slider action", "4"),
        ("Layout tab", "Layout page", "Layout action", "5"),
        ("Scroll tab", "Scroll page", "Scroll action", "6"),
        ("Custom tab", "Custom page", "Custom action", "7"),
    ] {
        let select = click(&adapter, tab);
        assert_eq!(adapter.update(select).unwrap(), program::Delivery::Applied);
        assert_text(&adapter, page);

        let child = click(&adapter, action);
        assert_eq!(adapter.update(child).unwrap(), program::Delivery::Applied);
        assert_text(&adapter, count);
    }
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn stale_mapped_component_messages_are_dropped_before_update() {
    let source = component_source("Message::Child");
    let old = boot(&source);
    let stale = click(&old, "Increment child");
    let mut current = boot(&source);

    let transition = current.update(stale).unwrap();

    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert_text(&current, "0");
    assert!(current.diagnostics().is_empty());
}

#[test]
fn free_and_associated_unary_functions_are_symbolic_mappers() {
    for mapper in ["lift", "Message::lift"] {
        let source = component_source(mapper);
        let mut adapter = boot(&source);
        let message = click(&adapter, "Increment child");

        assert_eq!(adapter.update(message).unwrap(), program::Delivery::Applied);
        assert_text(&adapter, "1");
    }
}

#[test]
fn invalid_symbolic_mapper_shapes_fail_during_compilation() {
    for (mapper, expected) in [
        ("Message::Idle", "found 'Message'"),
        ("Message::Pair", "found 'fn(i64, i64) -> Message'"),
        ("Other::Child", "Other"),
        ("wrong_input", "bool"),
    ] {
        let source = diagnostic_source(mapper);
        let Err(error) = compiler().compile_application_descriptor(&source) else {
            panic!("mapper `{mapper}` unexpectedly compiled");
        };
        let error = error.to_string();

        assert!(
            error.contains(expected),
            "mapper `{mapper}` diagnostic did not contain `{expected}`: {error}"
        );
    }
}

#[test]
fn retained_component_mappers_rebind_and_reject_signature_or_owner_drift() {
    let old = boot(&component_source("Message::Child"));
    let retained = old.display().clone();

    for (source, expected) in [
        (changed_signature_source(), "signature"),
        (changed_owner_source(), "owner"),
    ] {
        let mut current = boot(source);
        let context = current.context();
        let mut simulator = iced_test::simulator(retained.lower(&context));
        simulator.click("Increment child").unwrap();
        let message = simulator.into_messages().next().unwrap();

        let error = current.update(message).unwrap_err();

        assert!(matches!(error, program::Error::Mapping(_)));
        assert!(
            error.to_string().contains(expected),
            "stale mapper diagnostic did not name `{expected}` drift: {error}"
        );
        assert_eq!(current.diagnostics().len(), 1);
    }
}

#[test]
fn third_party_facades_can_accept_the_same_symbolic_callable_shape() {
    let mut engine = Engine::new();
    engine.register_host_facade(extension_facade()).unwrap();

    engine
        .compile(
            r#"
use kit::Carrier;
#[derive(Debug, Clone, Copy)] enum Local { Tick }
#[derive(Debug, Clone, Copy)] enum Parent { Child(Local) }
pub fn compose(value: Carrier<'_, Local>) -> Carrier<'_, Parent> {
    value.map(Parent::Child)
}
"#,
        )
        .unwrap();
}

fn boot(source: &str) -> program::Adapter {
    let engine = compiler();
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

fn compiler() -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    Shared::new(engine)
}

fn assert_text(adapter: &program::Adapter, expected: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(expected).unwrap();
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    simulator
        .into_messages()
        .next()
        .expect("the child button must emit one mapped message")
}

fn component_source(mapper: &str) -> String {
    r#"
use iced::Element;
use iced::widget::{button, column, text};

#[derive(Default)]
struct Counter { value: i64 }

#[derive(Debug, Clone, Copy)]
enum ChildMessage { Increment }

#[derive(Debug, Clone, Copy)]
enum Message { Child(ChildMessage) }

pub fn controls() -> Element<'_, ChildMessage> {
    button("Increment child").on_press(ChildMessage::Increment)
}

pub fn lift(message: ChildMessage) -> Message { Message::Child(message) }

impl Message {
    fn lift(message: ChildMessage) -> Message { Message::Child(message) }
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Child(_child) => self.value += 1,
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let child = controls().map(__MAPPER__);
        let current = text(self.value);
        column![
            child,
            current,
        ]
    }
}

pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
    .replace("__MAPPER__", mapper)
}

fn diagnostic_source(mapper: &str) -> String {
    format!(
        r#"
use iced::Element;
use iced::widget::button;
#[derive(Default)] struct Counter;
#[derive(Debug, Clone, Copy)] enum ChildMessage {{ Increment }}
#[derive(Debug, Clone, Copy)] enum Message {{ Child(ChildMessage), Idle, Pair(i64, i64) }}
#[derive(Debug, Clone, Copy)] enum Other {{ Child(ChildMessage) }}
pub fn wrong_input(value: bool) -> Message {{ Message::Idle }}
pub fn controls() -> Element<'_, ChildMessage> {{
    button("child").on_press(ChildMessage::Increment)
}}
impl Counter {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{
        let child = controls().map({mapper});
        child
    }}
}}
pub fn main() -> iced::Result {{ iced::run(Counter::update, Counter::view) }}
"#
    )
}

fn tabbed_tour_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::{button, column, container, row, text};

#[derive(Debug, Clone, Copy, PartialEq)] enum Tab { Start, TextInput, Choices, Slider, Layout, Scroll, Custom }
#[derive(Debug, Clone, Copy)] enum StartMessage { Act }
#[derive(Debug, Clone, Copy)] enum TextInputMessage { Act }
#[derive(Debug, Clone, Copy)] enum ChoicesMessage { Act }
#[derive(Debug, Clone, Copy)] enum SliderMessage { Act }
#[derive(Debug, Clone, Copy)] enum LayoutMessage { Act }
#[derive(Debug, Clone, Copy)] enum ScrollMessage { Act }
#[derive(Debug, Clone, Copy)] enum CustomMessage { Act }
#[derive(Debug, Clone, Copy)]
enum Message {
    SelectStart,
    SelectTextInput,
    SelectChoices,
    SelectSlider,
    SelectLayout,
    SelectScroll,
    SelectCustom,
    Start(StartMessage),
    TextInput(TextInputMessage),
    Choices(ChoicesMessage),
    Slider(SliderMessage),
    Layout(LayoutMessage),
    Scroll(ScrollMessage),
    Custom(CustomMessage),
}

struct State { tab: Tab, actions: i64 }

pub fn start() -> Element<StartMessage> { column![text("Start page"), button("Start action").on_press(StartMessage::Act)] }
pub fn text_input_page() -> Element<TextInputMessage> { column![text("Text input page"), button("Text input action").on_press(TextInputMessage::Act)] }
pub fn choices() -> Element<ChoicesMessage> { column![text("Choices page"), button("Choices action").on_press(ChoicesMessage::Act)] }
pub fn slider_page() -> Element<SliderMessage> { column![text("Slider page"), button("Slider action").on_press(SliderMessage::Act)] }
pub fn layout() -> Element<LayoutMessage> { column![text("Layout page"), button("Layout action").on_press(LayoutMessage::Act)] }
pub fn scroll() -> Element<ScrollMessage> { column![text("Scroll page"), button("Scroll action").on_press(ScrollMessage::Act)] }
pub fn custom() -> Element<CustomMessage> { column![text("Custom page"), button("Custom action").on_press(CustomMessage::Act)] }

impl State {
    fn new() -> State { State { tab: Tab::Start, actions: 0 } }
    fn update(&mut self, message: Message) {
        match message {
            Message::SelectStart => self.tab = Tab::Start,
            Message::SelectTextInput => self.tab = Tab::TextInput,
            Message::SelectChoices => self.tab = Tab::Choices,
            Message::SelectSlider => self.tab = Tab::Slider,
            Message::SelectLayout => self.tab = Tab::Layout,
            Message::SelectScroll => self.tab = Tab::Scroll,
            Message::SelectCustom => self.tab = Tab::Custom,
            Message::Start(_) => self.actions += 1,
            Message::TextInput(_) => self.actions += 1,
            Message::Choices(_) => self.actions += 1,
            Message::Slider(_) => self.actions += 1,
            Message::Layout(_) => self.actions += 1,
            Message::Scroll(_) => self.actions += 1,
            Message::Custom(_) => self.actions += 1,
        }
    }
    fn view(&self) -> Element<Message> {
        let start_style = if self.tab == Tab::Start { button::primary } else { button::secondary };
        let text_input_style = if self.tab == Tab::TextInput { button::primary } else { button::secondary };
        let choices_style = if self.tab == Tab::Choices { button::primary } else { button::secondary };
        let slider_style = if self.tab == Tab::Slider { button::primary } else { button::secondary };
        let layout_style = if self.tab == Tab::Layout { button::primary } else { button::secondary };
        let scroll_style = if self.tab == Tab::Scroll { button::primary } else { button::secondary };
        let custom_style = if self.tab == Tab::Custom { button::primary } else { button::secondary };
        let tabs = row![
            button("Start tab").style(start_style).on_press(Message::SelectStart),
            button("Text input tab").style(text_input_style).on_press(Message::SelectTextInput),
            button("Choices tab").style(choices_style).on_press(Message::SelectChoices),
            button("Slider tab").style(slider_style).on_press(Message::SelectSlider),
            button("Layout tab").style(layout_style).on_press(Message::SelectLayout),
            button("Scroll tab").style(scroll_style).on_press(Message::SelectScroll),
            button("Custom tab").style(custom_style).on_press(Message::SelectCustom),
        ];
        let content = match self.tab {
            Tab::Start => start().map(Message::Start),
            Tab::TextInput => text_input_page().map(Message::TextInput),
            Tab::Choices => choices().map(Message::Choices),
            Tab::Slider => slider_page().map(Message::Slider),
            Tab::Layout => layout().map(Message::Layout),
            Tab::Scroll => scroll().map(Message::Scroll),
            Tab::Custom => custom().map(Message::Custom),
        };
        container(column![tabs, content, text(self.actions).style(text::secondary)])
            .style(container::rounded_box)
    }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#
}

fn changed_signature_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::button;
#[derive(Default)] struct Counter;
#[derive(Debug, Clone, Copy)] enum Message { Child(bool) }
pub fn controls() -> Element<'_, bool> { button("new").on_press(true) }
impl Counter {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> {
        let child = controls().map(Message::Child);
        child
    }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

fn changed_owner_source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::button;
#[derive(Default)] struct Counter;
#[derive(Debug, Clone, Copy)] enum ChildMessage { Increment }
#[derive(Debug, Clone, Copy)] enum Other { Child(ChildMessage) }
pub fn controls() -> Element<'_, ChildMessage> {
    button("new").on_press(ChildMessage::Increment)
}
impl Counter {
    fn update(&mut self, message: Other) {}
    fn view(&self) -> Element<'_, Other> {
        let child = controls().map(Other::Child);
        child
    }
}
pub fn main() -> iced::Result { iced::run(Counter::update, Counter::view) }
"#
}

#[derive(Clone)]
struct ExtensionCarrier;

fn extension_facade() -> host::facade::Definition {
    let mut builder =
        host::facade::Builder::new("kit", host::facade::Abi::new(1).unwrap()).unwrap();
    builder
        .type_::<ExtensionCarrier>("Carrier", &["Message"], &[])
        .unwrap();
    let mut signature = host::signature::Builder::new();
    signature.type_parameter("Message", &[]).unwrap();
    signature.type_parameter("Parent", &[]).unwrap();
    signature.parameter(host::type_ref::Type::callable(
        host::type_ref::Type::parameter("Message").unwrap(),
        host::type_ref::Type::parameter("Parent").unwrap(),
    ));
    signature.output(
        host::type_ref::Type::applied(
            "Carrier",
            [host::type_ref::Type::parameter("Parent").unwrap()],
        )
        .unwrap(),
    );
    builder
        .item(
            host::item::Definition::method(
                "Carrier::map",
                rawr::receiver::Mode::Owned,
                signature.finish(),
            )
            .unwrap(),
        )
        .unwrap();
    builder.finish().unwrap()
}
