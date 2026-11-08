//! Native Rust: the root declares a native `page` module and the marked
//! `component` module, whose marked `badge` names `page`'s items.
mod component;
pub mod page;

use component::badge;

fn main() {
    println!("{}", badge::keep(Box::new(page::Home)).title());
    for wide in [false, true] {
        let shown = badge::pick(Box::new(page::Home), Box::new(page::About), wide);
        println!("{}", shown.title());
    }
    println!("{}", badge::scaled(2));
    let width: page::size::Width = badge::widen(4);
    println!("{width}");
    println!("{}", badge::measure(&5));
    println!("{}", badge::titled(page::Home));
}
