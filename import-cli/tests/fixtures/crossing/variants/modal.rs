// rawr
// iced's modal (#955): a script function generic over its widgets' types
// and their lifetime, `modal<'a, Message>(base: impl Widget<Message> + 'a,
// ..) -> impl Widget<Message> + 'a`, puts the content in an opaque widget,
// a container and a mouse area, whose types name that lifetime. The
// content's field borrows the state's email for the view's lifetime, as
// `Field<'a, ..>::value(self, &'a str) -> Self` says (#598). A press
// outside the content hides it again.

use events::controls::{application, toggle};
use events::widgets::{button, column, container, field, mouse_area, opaque, Widget};

#[derive(Default)]
struct App {
    show_modal: bool,
    shown: u32,
    email: String,
}

#[derive(Debug, Clone)]
enum Message {
    ShowModal,
    HideModal,
    Submit,
    Toggled(bool),
}

impl App {
    fn update(&mut self, message: Message) {
        match message {
            Message::ShowModal => {
                self.show_modal = true;
                self.shown += 1;
            }
            Message::HideModal => self.show_modal = false,
            Message::Submit => self.email = format!("me{}@rawr", self.shown),
            Message::Toggled(shown) => self.show_modal = shown,
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let content = container(column![
            button("Show").on_press(Message::ShowModal),
            toggle(self.show_modal)
                .on_toggle(Message::Toggled)
                .label("Shown"),
        ])
        .padding(10);

        if self.show_modal {
            let signup = container(column![
                field("Email").value(&self.email).on_submit(Message::Submit),
                button("Submit").on_press(Message::HideModal),
            ])
            .padding(self.shown as u16);

            modal(content, signup, Message::HideModal).boxed()
        } else {
            content.boxed()
        }
    }
}

fn modal<'a, Message>(
    base: impl Widget<Message> + 'a,
    content: impl Widget<Message> + 'a,
    on_blur: Message,
) -> impl Widget<Message> + 'a
where
    Message: Clone + 'a,
{
    column![
        base,
        opaque(mouse_area(container(opaque(content)).padding(2)).on_press(on_blur))
    ]
}

fn main() {
    application(App::default, App::update, App::view).run()
}
