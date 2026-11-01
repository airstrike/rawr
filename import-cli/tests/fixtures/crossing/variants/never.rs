// rawr
// iced's geometry: a view that sends no message is `impl Widget<Never>`,
// where iced's `Never` is std's `Infallible`. Its body nests containers
// around a column, and the type it reveals names `Infallible` as the
// boxed children's message, which the runner writes and the crate's
// generic items take as their type argument.

use events::widgets::{center, column, text, Widget};
use std::convert::Infallible as Never;

#[derive(Default)]
struct Geometry;

fn view(_state: &Geometry) -> impl Widget<Never> {
    let content = column![
        text("rainbow"),
        "In this example we draw a custom widget Rainbow",
    ]
    .padding(20);

    let scrollable = center(center(content));

    center(scrollable)
}

fn update(_state: &mut Geometry, _message: Never) {}

fn main() {
    events::widgets::run(update, view);
}
