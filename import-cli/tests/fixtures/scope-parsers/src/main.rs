// rawr
//! Two pull parsers whose values borrow what they parse, written as each
//! crate's docs write them (D558 slice 2): pulldown-cmark's `for event in
//! Parser::new(&text)` and quick-xml's `read_event_into` loop. Each value
//! lives in a native scope frame for the Rust scope it is bound in.

mod buffer;
mod events;
mod frames;
mod reader;

fn main() {
    events::main();
    reader::main();
    buffer::main();
    frames::main();
}
