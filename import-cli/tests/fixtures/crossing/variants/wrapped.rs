// rawr
// iced's modal (#955), without what borrows the view: a script function
// generic over its widget's type and lifetime, `wrapped<'a, Message>(content:
// impl Widget<Message> + 'a, ..) -> impl Widget<Message> + 'a`, puts a rule
// in an opaque widget, a container and a mouse area, whose types name that
// lifetime, and that in another opaque widget. The runner names each type
// with its lifetimes, as rustc does. Beside it, a field borrows the state's
// email for the view's lifetime, as `Field<'a, ..>::value(self, &'a str) ->
// Self` says (#598).

use events::controls::application;
use events::widgets::{column, container, field, mouse_area, opaque, rule, Widget};

#[derive(Default)]
struct App {
    width: u16,
    email: String,
}

#[derive(Debug, Clone)]
enum Message {
    Widen,
    Submit,
    Blurred,
}

impl App {
    fn update(&mut self, message: Message) {
        match message {
            Message::Widen => self.width += 1,
            Message::Submit => self.email = format!("me{}@rawr", self.width),
            Message::Blurred => self.width = 0,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        column![
            field("Email").value(&self.email).on_submit(Message::Submit),
            wrapped(rule(self.width, Message::Widen), Message::Blurred),
        ]
    }
}

fn wrapped<'a, Message>(
    content: impl Widget<Message> + 'a,
    on_blur: Message,
) -> impl Widget<Message> + 'a
where
    Message: Clone + 'a,
{
    opaque(mouse_area(container(opaque(content)).padding(2)).on_press(on_blur))
}

fn main() {
    application(App::default, App::update, App::view).run()
}
