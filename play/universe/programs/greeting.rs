//! A text input that greets what it is given (#2312): a value typed in,
//! one message for each edit and one for Enter, and text of the value.
//! In a browser the page's keys type into it. On the server, which has no
//! keyboard, `typed` types into it with iced_test's simulator, as iced's
//! todos test types a new task.

use iced::widget::{column, text, text_input, Widget};
use iced::Center;

#[derive(Default)]
struct Greeting {
    name: String,
    greeted: bool,
}

#[derive(Debug, Clone)]
enum Message {
    NameChanged(String),
    Greet,
}

impl Greeting {
    fn update(&mut self, message: Message) {
        match message {
            Message::NameChanged(name) => {
                self.name = name;
                self.greeted = false;
            }
            Message::Greet => self.greeted = true,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let greeting = if self.greeted {
            format!("Hello, {}!", self.name)
        } else {
            String::from("Type a name and press Enter")
        };
        column![
            text_input("Your name", &self.name)
                .id("name")
                .on_input(Message::NameChanged)
                .on_submit(Message::Greet)
                .padding(10)
                .size(20),
            text(greeting).size(30),
        ]
        .spacing(20)
        .padding(20)
        .align_x(Center)
    }
}

pub fn main() -> iced::Result {
    iced::run(Greeting::update, Greeting::view)
}

/// What the greeting says once `name` is typed into its input and Enter
/// pressed: a click focuses the input, each typed key is an edit, and the
/// messages are performed in the order the input sent them.
#[cfg(not(target_arch = "wasm32"))]
pub fn typed(name: &str) -> Result<String, iced_test::Error> {
    use iced::keyboard::key;
    use iced_test::selector::id;
    use iced_test::simulator;

    let mut greeting = Greeting::default();
    let mut ui = simulator(greeting.view());
    let _input = ui.click(id("name"))?;
    let _ = ui.typewrite(name);
    let _ = ui.tap_key(key::Named::Enter);
    for message in ui.into_messages() {
        greeting.update(message);
    }
    let mut ui = simulator(greeting.view());
    let said = format!("Hello, {}!", greeting.name);
    let _ = ui.find(said.as_str())?;
    Ok(said)
}
