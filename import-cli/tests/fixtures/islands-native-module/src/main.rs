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
    let mut by: page::size::Width = 4;
    let mut tally = page::Tally::new();
    let mut again: page::size::Width = 2;
    badge::grow(&mut by, &mut tally, &mut again);
    badge::grow(&mut by, &mut tally, &mut again);
    println!("{by} {} {again}", tally.count());
}
