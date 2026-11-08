//! Native Rust: the root declares a native `page` module and the marked
//! `component` module, whose marked `badge` writes its page as `page`'s
//! blocks.
mod component;
pub mod page;

use component::badge;

fn main() {
    page::lay_out(badge::BLOCKS, 0);
    page::lay_out(&[badge::note()], 0);
    badge::show();
    println!("{}", badge::heading());
    println!("{}", badge::BLOCKS.len());
    println!("{}", badge::count());
    badge::titles();
    badge::state().show();
    badge::part().show();
    badge::promoted().show();
    let page = badge::BadgePage::new(7);
    page::describe(&*page);
    println!(
        "{} {} {}",
        std::ptr::eq(page.meta(), &badge::META),
        std::ptr::eq(page.blocks(), badge::BLOCKS),
        std::ptr::eq(page.state(), page.state())
    );
}
