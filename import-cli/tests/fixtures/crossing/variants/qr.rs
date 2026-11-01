// rawr
// iced's qr_code: `qr_code(data)` borrows the state's code data for the
// view's lifetime, as its `fn qr_code<'a, Theme>(data: &'a Data) ->
// QRCode<'a, Theme>` says. The data isn't `Clone` and counts its draws, so
// each view draws the one value the state holds, which `update` then
// changes in place once no view borrows it.

use events::controls::{qr_code, toggle};
use events::widgets::{column, Widget};

struct Generator {
    qr_code: Option<qr_code::Data>,
    total_size: Option<f32>,
}

impl Default for Generator {
    fn default() -> Self {
        Self {
            qr_code: qr_code::Data::new("rawr").ok(),
            total_size: None,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ToggleTotalSize(bool),
}

impl Generator {
    fn update(&mut self, message: Message) {
        match message {
            Message::ToggleTotalSize(enabled) => {
                self.total_size = enabled.then_some(220.0);
                if let Some(data) = &mut self.qr_code {
                    data.grow();
                }
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let toggle_total_size = toggle(self.total_size.is_some())
            .on_toggle(Message::ToggleTotalSize)
            .label("Limit");

        column![
            toggle_total_size,
            self.qr_code.as_ref().map(|data| {
                if let Some(total_size) = self.total_size {
                    qr_code(data).total_size(total_size)
                } else {
                    qr_code(data).cell_size(10.0)
                }
            })
        ]
    }
}

fn main() {
    events::controls::application(Generator::default, Generator::update, Generator::view).run()
}
