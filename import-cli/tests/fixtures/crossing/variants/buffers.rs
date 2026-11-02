// rawr
// Buffers a crate call writes into, as pulldown-cmark's `push_html(&mut s,
// ..)` and quick-xml's `read_event_into(&mut buf)` take them: the program's
// own `String` and `Vec<u8>`, lent in place as rustc lends them, so the
// room one call reserves is still there at the next, and the program reads
// what the calls wrote. A trait the crate implements for `&str` of any
// lifetime is called on the program's strings, as iced's `impl Widget for
// &str` is.

use events::buffers::{byte_room, fill, reserve, reserve_bytes, room, write_line, Label};

fn main() {
    let mut page = String::new();
    reserve(&mut page, 256);
    write_line(&mut page, "first");
    write_line(&mut page, "second");
    print!("{page}");
    println!("{} bytes, room kept {}", page.len(), room(&mut page) >= 256);
    page.push_str("third\n");
    write_line(&mut page, "fourth");
    print!("{page}");
    println!("room still kept {}", room(&mut page) >= 256);

    let mut buffer: Vec<u8> = Vec::new();
    reserve_bytes(&mut buffer, 64);
    fill(&mut buffer, 7, 3);
    fill(&mut buffer, 9, 2);
    let kept = byte_room(&mut buffer) >= 64;
    println!("{:?}, room kept {kept}", buffer);
    buffer.clear();
    fill(&mut buffer, 1, 1);
    let kept = byte_room(&mut buffer) >= 64;
    println!("{:?} {}, room kept {kept}", buffer, buffer.len());

    let names = [String::from("Quartz"), String::from("Basalt")];
    for name in &names {
        let width = Label::width(name.as_str());
        let shown = Label::shown(name.as_str());
        println!("{} is {width} wide", shown.text());
    }
    println!("{} wide", Label::width("literal"));
}
