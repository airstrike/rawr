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
    // `Page: Debug + PageClone`: the box prints by the page's `Debug`, and
    // its clone is a page of its own.
    println!("{:?}", pages[1]);
    let mut copy = pages[1].clone();
    copy.update(Message::Bump(10));
    println!("{:?} {:?}", pages[1], copy);
    // Results that borrow a page, read after their calls, while the page
    // is shared; it is reached `&mut` once they are gone.
    let label = pages[1].label();
    let items = pages[1].items();
    for page in &pages {
        println!("{} {:?}", page.label(), page.items());
    }
    println!("{label} {items:?} {}", pages[1].title());
    pages[1].update(Message::Rename("held".to_string()));
    println!("{} {:?}", pages[1].label(), pages[1].items());
    // A label read many times is the page's own text each time, and what
    // the page draws borrows that same text, however often it is drawn and
    // renamed (#1322, #1321).
    let first = pages[1].label().as_ptr();
    let mut same = true;
    for round in 0..200 {
        same &= std::ptr::eq(pages[1].label().as_ptr(), first);
        let drawn = pages[1].draw();
        same &= std::ptr::eq(drawn.text().as_ptr(), first);
        if round % 50 == 49 {
            println!("{} {same}", drawn.text());
        }
    }
    for round in 0..100 {
        pages[1].update(Message::Rename(format!("name {round}")));
        let label = pages[1].label();
        let drawn = pages[1].draw();
        same &= std::ptr::eq(drawn.text().as_ptr(), label.as_ptr());
        same &= drawn.text() == format!("name {round}");
    }
    println!(
        "{} {same} {}",
        pages[1].draw().text(),
        pages[0].draw().text()
    );
    pages[1].update(Message::Rename("held".to_string()));
    pages.push(copy);
    page::SHOWN.with_borrow_mut(|shown| shown.extend(pages));
    page::SHOWN.with_borrow(|shown| {
        for index in 0..shown.len() {
            println!("{}", shown[index].summary(index));
        }
        println!("{}", shown[1].summary(2));
    });
    println!("{}", badge::described(7));
}
