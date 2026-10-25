//! Widgets that borrow app state (D546): a `text_editor::Content` and a
//! `combo_box::State` the State holds, in the application's native home.

use iced::advanced::renderer::Headless as _;
use iced::keyboard::{self, key};
use iced::{mouse, Event, Point, Size};
use iced_runtime::user_interface::{self, UserInterface};

use super::carry_tests::boot;
use super::Adapter;

/// An editor over the State's `Content`, and a line showing its text and
/// the cursor's column.
const EDITOR: &str = r#"
use iced::widget::{column, text, text_editor};
use iced::Element;

struct State {
    content: text_editor::Content,
}

impl Default for State {
    fn default() -> Self {
        State { content: text_editor::Content::new() }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Edit(text_editor::Action),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Edit(action) => self.content.perform(action),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let cursor = self.content.cursor();
        column![
            text_editor(&self.content).on_action(Message::Edit).height(100),
            text(format!("VIEW {:?} at {}", self.content.text(), cursor.position.index)),
        ]
    }
}

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
"#;

/// iced's runtime over an adapter, as a window runs it: each batch of
/// events goes to an interface built over the adapter's view, which is
/// then kept only as iced's cache while the adapter applies the messages.
struct Window {
    renderer: iced::Renderer,
    cache: user_interface::Cache,
}

impl Window {
    fn new() -> Self {
        let renderer = iced::futures::executor::block_on(iced::Renderer::new(
            iced::advanced::renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("the pinned tiny-skia renderer must be available");
        Self {
            renderer,
            cache: user_interface::Cache::default(),
        }
    }

    fn send(&mut self, adapter: &mut Adapter, events: &[Event]) {
        let mut interface = UserInterface::build(
            adapter.view(),
            Size::new(400.0, 300.0),
            std::mem::take(&mut self.cache),
            &mut self.renderer,
        );
        let mut messages = iced::advanced::shell::Bus::new();
        let _ = interface.update(
            &iced::window::Headless,
            &iced::advanced::shell::Waker::noop(),
            events,
            mouse::Cursor::Available(Point::new(20.0, 20.0)),
            &mut self.renderer,
            &mut messages,
        );
        self.cache = interface.into_cache();
        for (message, _) in messages.drain() {
            adapter.update(message).unwrap();
        }
    }

    fn click(&mut self, adapter: &mut Adapter) {
        self.send(
            adapter,
            &[
                Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(20.0, 20.0),
                }),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ],
        );
    }

    fn tap(&mut self, adapter: &mut Adapter, key: keyboard::Key, modifiers: keyboard::Modifiers) {
        let text = match &key {
            keyboard::Key::Character(text) if !modifiers.command() => Some(text.clone()),
            _ => None,
        };
        let physical = key::Physical::Unidentified(key::NativeCode::Unidentified);
        self.send(
            adapter,
            &[
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: key.clone(),
                    modified_key: key.clone(),
                    physical_key: physical,
                    location: keyboard::Location::Standard,
                    modifiers,
                    text,
                    repeat: false,
                }),
                Event::Keyboard(keyboard::Event::KeyReleased {
                    key: key.clone(),
                    modified_key: key,
                    physical_key: physical,
                    location: keyboard::Location::Standard,
                    modifiers,
                }),
            ],
        );
    }

    fn typewrite(&mut self, adapter: &mut Adapter, text: &str) {
        for character in text.chars() {
            let key = keyboard::Key::Character(character.to_string().into());
            self.tap(adapter, key, keyboard::Modifiers::default());
        }
    }
}

fn shows(adapter: &Adapter, line: &str) -> bool {
    iced_test::simulator(adapter.view()).find(line).is_ok()
}

#[test]
fn a_text_editor_edits_the_content_the_state_holds_and_carries_it() {
    let mut old = boot(EDITOR);
    let mut window = Window::new();
    window.click(&mut old);
    window.typewrite(&mut old, "abc");
    assert!(shows(&old, r#"VIEW "abc" at 3"#));
    // Select "bc", then type over it.
    for _ in 0..2 {
        window.tap(
            &mut old,
            keyboard::Key::Named(key::Named::ArrowLeft),
            keyboard::Modifiers::SHIFT,
        );
    }
    window.typewrite(&mut old, "X");
    assert!(shows(&old, r#"VIEW "aX" at 2"#));

    // An edit to the view's body keeps the State, and the same `Content`
    // with its text, cursor and undo history.
    let edited = EDITOR.replace("VIEW", "EDITED");
    let mut new = boot(&edited);
    assert!(new.carry_state(&mut old));
    assert!(shows(&new, r#"EDITED "aX" at 2"#));
    window.tap(
        &mut new,
        keyboard::Key::Character("z".into()),
        keyboard::Modifiers::COMMAND,
    );
    // Undo takes back the typed "X" over the selection.
    assert!(shows(&new, r#"EDITED "abc" at 3"#));
}

/// A `combo_box::State` the update replaces each time.
const COMBO: &str = r#"
use iced::widget::{button, column, combo_box, text};
use iced::Element;

struct State {
    choices: combo_box::State<String>,
    resets: u32,
}

impl Default for State {
    fn default() -> Self {
        State { choices: combo_box::State::new(vec!["a".to_string(), "b".to_string()]), resets: 0 }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Reset,
    Picked(String),
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Reset => {
                self.choices = combo_box::State::new(vec!["c".to_string()]);
                self.resets += 1;
            }
            Message::Picked(_) => {}
        }
    }

    fn view(&self) -> Element<'_, Message> {
        column![
            button(text("Reset")).on_press(Message::Reset),
            combo_box(&self.choices, "Pick", None, Message::Picked),
            text(format!("{} resets", self.resets)),
        ]
    }
}

pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
"#;

#[test]
fn a_replaced_combo_box_state_is_freed_with_its_last_handle() {
    let mut adapter = boot(COMBO);
    assert!(shows(&adapter, "0 resets"));
    for resets in 1..=5 {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.click("Reset").unwrap();
        let message = simulator.into_messages().next().unwrap();
        adapter.update(message).unwrap();
        assert!(shows(&adapter, &format!("{resets} resets")));
    }
    // The State's own and, until the next transition, the last view's.
    assert!(adapter.home.len() <= 2, "{} slots", adapter.home.len());
}
