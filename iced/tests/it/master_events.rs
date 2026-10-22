//! iced master's runtime-event subscriptions: `keyboard::listen()` with
//! `filter_map`, `event::listen()`, `window::frames()` and `time::every`.

use iced::futures::StreamExt;
use rawr_iced::{program, runtime};

fn boot(source: &str) -> program::Adapter {
    rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(source)
        .unwrap_or_else(|error| panic!("{error}"))
        .prepare()
        .unwrap()
        .into_adapter()
}

const HOTKEYS: &str = r#"
use iced::keyboard;
use iced::keyboard::key;
use iced::time::{self, Duration};
use iced::widget::{column, text};
use iced::{event, Event, Subscription, Widget};

pub fn main() -> iced::Result {
    iced::application(State::default, State::update, State::view)
        .subscription(State::subscription)
        .run()
}

#[derive(Default)]
struct State {
    toggles: i64,
    resets: i64,
    events: i64,
}

#[derive(Debug, Clone)]
enum Message {
    Toggle,
    Reset,
    Tick,
    Event(Event),
}

fn handle_hotkey(event: keyboard::Event) -> Option<Message> {
    match event {
        keyboard::Event::KeyPressed { modified_key, modifiers, .. } => {
            if modified_key == keyboard::Key::Named(key::Named::Space) {
                Some(Message::Toggle)
            } else if modified_key == keyboard::Key::Character("r") && !modifiers.shift() {
                Some(Message::Reset)
            } else {
                None
            }
        }
        _ => None,
    }
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggle => {
                self.toggles += 1;
            }
            Message::Reset => {
                self.resets += 1;
            }
            Message::Tick => {}
            Message::Event(_) => {
                self.events += 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![text(self.toggles), text(self.resets), text(self.events)]
    }

    fn subscription(&self) -> Subscription<Message> {
        let tick = time::every(Duration::from_millis(10)).map(|_| Message::Tick);
        Subscription::batch(vec![
            tick,
            keyboard::listen().filter_map(handle_hotkey),
            event::listen().map(Message::Event),
        ])
    }
}
"#;

fn press(key: iced::keyboard::Key) -> iced::advanced::subscription::Event {
    iced::advanced::subscription::Event::Interaction {
        window: iced::window::Id::unique(),
        event: iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        }),
        status: iced::event::Status::Ignored,
    }
}

#[test]
fn master_keyboard_and_event_subscriptions_deliver_messages() {
    let adapter = boot(HOTKEYS);
    let application = program::Application::from_adapter(adapter);
    let (mut state, _) = <program::Application as iced::Program>::boot(&application);
    let subscription = <program::Application as iced::Program>::subscription(&application, &state);
    let recipes = iced::advanced::subscription::into_recipes(subscription);
    assert_eq!(recipes.len(), 3);
    let events = || {
        Box::pin(iced::futures::stream::iter([
            press(iced::keyboard::Key::Named(iced::keyboard::key::Named::Space)),
            press(iced::keyboard::Key::Character("r".into())),
            press(iced::keyboard::Key::Character("x".into())),
        ])) as iced::advanced::subscription::EventStream
    };
    let mut messages: Vec<runtime::Message> = Vec::new();
    for recipe in recipes {
        let mut stream = recipe.stream(events());
        let taken = iced::futures::executor::block_on(async {
            let mut taken = Vec::new();
            // The timer never ends; take one tick from it.
            while let Some(message) = stream.next().await {
                taken.push(message);
                if taken.len() == 3 {
                    break;
                }
            }
            taken
        });
        messages.extend(taken);
    }
    // Three ticks, Toggle and Reset, and three events.
    assert_eq!(messages.len(), 8);
    for message in messages {
        let _ = <program::Application as iced::Program>::update(&application, &mut state, message);
    }
    let view = <program::Application as iced::Program>::view(
        &application,
        &state,
        iced::window::Id::unique(),
    );
    let mut simulator = iced_test::simulator(view);
    simulator.find("3").unwrap();
}
