//! Native Rust: the root keeps its pages as trait objects, some written
//! natively and some by the marked `badge`, and calls their methods.
mod component;
pub mod page;

use component::badge;
use page::{Message, Page};

fn main() {
    let mut pages: Vec<Box<dyn Page>> = vec![Box::new(page::Home), badge::new(1), badge::new(2)];
    for page in &pages {
        println!("{}: {}", page.title(), page.view());
    }
    println!("{:?}", pages[1].update(Message::Bump(3)));
    println!("{:?}", pages[1].update(Message::Bump(4)));
    println!(
        "{:?}",
        pages[2].update(Message::Rename("count".to_string()))
    );
    for page in &mut pages {
        page.update(Message::Bump(1));
    }
    for page in &pages {
        println!("{}: {}", page.title(), page.view());
    }
    pages.remove(1);
    println!("{}", pages.len());
    for page in &pages {
        println!("{}: {}", page.title(), page.view());
    }
}
