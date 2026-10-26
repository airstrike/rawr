// rawr
//! Where a frame opens and ends: a statement that calls a method on the
//! value, a closure's body, the right side of `&&`, a loop's condition, a
//! `while let` and a value a function returns.

use pulldown_cmark::{Event, Parser, TagEnd};
use quick_xml::events::Event as XmlEvent;
use quick_xml::reader::Reader;

fn ends(text: &str) -> usize {
    Parser::new(text)
        .filter(|event| matches!(event, Event::End(_)))
        .count()
}

pub fn main() {
    let text = String::from("*a* and **b**\n\n- c\n");
    let all = Parser::new(&text).count();
    println!("events: {all}, ends: {}", ends(&text));

    // What comes after the borrowing call in its statement, or isn't a
    // call, runs where Rust runs it.
    let sizes = 1 + Parser::new(&text).count() + text.len();
    println!("sizes: {sizes}");

    let words = |line: &str| {
        Parser::new(line)
            .filter(|event| matches!(event, Event::Text(_)))
            .count()
    };
    println!("texts: {}", words("x *y* z"));

    if !text.is_empty() && Parser::new(&text).any(|event| event == Event::End(TagEnd::Item)) {
        println!("has an item");
    }

    let mut rounds = 0;
    while Parser::new(&text).count() > rounds * 4 {
        rounds += 1;
    }
    println!("rounds: {rounds}");

    let mut reader = Reader::from_str("<a>1</a><b>2</b>");
    let mut names = Vec::new();
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) => {
                names.push(String::from_utf8(e.name().as_ref().to_vec()).unwrap())
            }
            XmlEvent::Eof => break,
            _ => (),
        }
    }
    println!("names: {names:?}");
}
