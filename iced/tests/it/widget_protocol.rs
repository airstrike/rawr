use rawr::widget::{hook, message, state};
use rawr::{host, Engine, Shared};

#[test]
fn installed_facade_compiles_the_complete_scripted_widget_protocol() {
    let source = r#"
        use iced::Color;

        struct State { clicks: i64 }
        struct Dial { radius: f64 }
        enum Message { Changed(i64) }

        #[widget]
        impl Dial {
            fn state(&self) -> State { panic!("state") }
            fn children(&self) -> rawr_iced::widget::children::List<Message> { panic!("children") }
            fn diff(&mut self, state: &mut State) { () }
            fn size(&self) -> rawr_iced::widget::layout::Size { panic!("size") }
            fn layout(&mut self, state: &mut State, input: rawr_iced::widget::layout::Input) -> rawr_iced::widget::layout::Node { panic!("layout") }
            fn update(&mut self, state: &mut State, input: rawr_iced::widget::event::Input) -> rawr_iced::widget::event::Outcome<Message> { panic!("update") }
            fn draw(&self, state: &State, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                renderer.fill_quad(input.bounds(), Color::from_rgb(0.95, 0.45, 0.10));
                renderer.fill_rounded_quad(input.bounds(), Color::from_rgb(0.95, 0.45, 0.10), 12);
            }
            fn mouse_interaction(&self, state: &State, input: rawr_iced::widget::mouse::Input) -> iced::mouse::Interaction { panic!("mouse") }
            fn operate(&mut self, state: &mut State, input: rawr_iced::widget::operation::Input) -> rawr_iced::widget::operation::Outcome { panic!("operate") }
            fn overlay(&mut self, state: &mut State, input: rawr_iced::widget::overlay::Input) -> rawr_iced::widget::overlay::Value<Message> { panic!("overlay") }
        }
    "#;
    let mut engine = Engine::new();
    let iced = rawr_iced::facade().unwrap();
    let expected = iced.fingerprint().clone();
    let lifecycle = rawr_iced::extension(&iced).unwrap().fingerprint().clone();
    rawr_iced::install(&mut engine).unwrap();
    let executable = engine.compile_executable(source).unwrap();
    let [protocol] = executable.widgets() else {
        panic!("expected one widget protocol");
    };

    assert!(matches!(protocol.state(), state::Type::Nominal(_)));
    assert!(matches!(protocol.message(), message::Type::Exact(_)));
    assert_eq!(protocol.dependency().fingerprint(), &expected);
    assert_eq!(protocol.lifecycle().fingerprint(), &lifecycle);
    assert_eq!(protocol.hooks().len(), 10);
    assert!(protocol.hook(hook::Kind::Overlay).is_some());
}

#[test]
fn scripted_widget_launch_requires_both_exact_runtime_facades() {
    let source = r#"
        use iced::Element;

        #[derive(Default)] struct State;
        struct Dial;
        #[derive(Debug, Clone, Copy)] enum Message { Tick }

        #[widget]
        impl Dial {}

        impl State {
            fn update(&mut self, message: Message) {
                match message { Message::Tick => () }
            }
            fn view(&self) -> Element<Message> { Dial {} }
        }

        pub fn main() -> iced::Result { iced::run(State::update, State::view) }
    "#;
    let launch = || {
        let mut engine = Engine::new();
        rawr_iced::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        engine.launch_application(descriptor).unwrap()
    };

    let [iced, _rawr] = rawr_iced::runtime_facades().unwrap();
    let mut missing_rawr = host::runtime::Registry::new();
    missing_rawr.install(iced).unwrap();
    assert!(matches!(
        launch().bind(missing_rawr),
        Err(host::runtime::Error::MissingFacade { facade }) if facade == "rawr_iced"
    ));

    let [_iced, rawr] = rawr_iced::runtime_facades().unwrap();
    let mut missing_iced = host::runtime::Registry::new();
    assert!(matches!(
        missing_iced.install(rawr),
        Err(host::runtime::Error::MissingFacade { facade }) if facade == "iced"
    ));
    assert!(matches!(
        launch().bind(missing_iced),
        Err(host::runtime::Error::MissingFacade { facade }) if facade == "iced"
    ));

    let [_iced, rawr] = rawr_iced::runtime_facades().unwrap();
    let stale_iced = host::facade::Builder::new("iced", host::facade::Abi::new(13).unwrap())
        .unwrap()
        .finish()
        .unwrap();
    let mut stale = host::runtime::Registry::new();
    stale
        .install(host::runtime::Builder::new(stale_iced).finish())
        .unwrap();
    assert!(matches!(
        stale.install(rawr),
        Err(host::runtime::Error::FacadeMismatch { facade }) if facade == "iced"
    ));
    let substitute = host::facade::Builder::new("rawr_iced", host::facade::Abi::new(2).unwrap())
        .unwrap()
        .finish()
        .unwrap();
    stale
        .install(host::runtime::Builder::new(substitute).finish())
        .unwrap();

    let [iced, _rawr] = rawr_iced::runtime_facades().unwrap();
    let incompatible = host::facade::Builder::new("rawr_iced", host::facade::Abi::new(2).unwrap())
        .unwrap()
        .finish()
        .unwrap();
    let mut mismatched = host::runtime::Registry::new();
    mismatched.install(iced).unwrap();
    mismatched
        .install(host::runtime::Builder::new(incompatible).finish())
        .unwrap();
    assert!(matches!(
        launch().bind(mismatched),
        Err(host::runtime::Error::FacadeMismatch { facade }) if facade == "rawr_iced"
    ));

    let mut production = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        production.install(facade).unwrap();
    }
    launch().bind(production).unwrap();
}
