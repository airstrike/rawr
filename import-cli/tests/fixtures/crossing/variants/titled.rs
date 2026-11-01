// rawr
// fount's title in the fixture's controls: `.title("fount – system font")`
// takes `impl Title<State>`, which the crate implements for `&'static str`,
// so the literal lives for the program, as rustc's literals do. A view
// calls `Named::with_name(&'static str)` every frame, and a loop calls
// `heading("..")` through the same impl, so each site holds its literal
// once however often it runs (#667).

use events::controls::{caption, heading, toggle, Named};
use events::widgets::{column, Widget};

#[derive(Default)]
struct Titled {
    count: u32,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Count(bool),
}

impl Titled {
    fn update(&mut self, message: Message) {
        match message {
            Message::Count(on) => self.count += u32::from(on),
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let font = Named::with_name("Fira Sans");
        let count = toggle(false).label("Count").on_toggle(Message::Count);
        column![count, caption(self.count), caption(font.len() as u32)]
    }
}

fn main() {
    let mut total = 0;
    for _ in 0..3 {
        total += heading("chapter").len();
        total += Named::with_name("Georgia").len();
    }
    println!("headings {total}");
    events::controls::application(Titled::default, Titled::update, Titled::view)
        .title("fount – system font")
        .run()
}
