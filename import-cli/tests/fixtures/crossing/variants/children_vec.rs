// rawr
// An array of Elements is no `Vec` of them: only a crate call's `impl
// IntoIterator<Item = Element>` takes it, so a `let` that annotates a
// `Vec` is rustc's E0308 (#737).

use events::widgets::{span, Element, Widget};

#[derive(Debug, Clone)]
enum Message {}

fn main() {
    let left: Element<'_, Message> = span("left").boxed();
    let right: Element<'_, Message> = span("right").boxed();
    let children: Vec<Element<'_, Message>> = [left, right];
    println!("{}", children.len());
}
