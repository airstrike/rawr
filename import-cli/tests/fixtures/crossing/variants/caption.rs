// rawr
// A crate struct's `String` field, as fount's `Font::family`, is read in
// place, as Rust reads it: `format!` takes it, a method borrows it, `&`
// lends it and `==` compares it, and the struct stays whole for the next
// read. A by-value use moves only that field, and its other fields stay
// usable, as Rust's partial move leaves them.

use events::conversions::{Caption, Pixels};

fn main() {
    let caption = Caption {
        content: String::from("quartz"),
        size: Pixels(16.0),
    };
    let shown = format!("[{}] {:?}", caption.content, caption.size);
    let length = caption.content.len();
    let lent = &caption.content;
    let same = caption.content == "quartz";
    println!("{shown} {length} {lent} {same}");
    let whole = caption.clone();
    let content = caption.content;
    println!("{content} {:?} {}", caption.size, whole.content);
}
