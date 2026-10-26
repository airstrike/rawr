// rawr
//! What a frame gives back: `read_event_into` fills the buffer, which holds
//! the event's bytes once the event is dropped, and `read_event` borrows
//! the input of a `Reader<&[u8]>` directly.

use quick_xml::events::Event;
use quick_xml::reader::Reader;

pub fn main() {
    let xml = "<a>one</a><b/>";
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    match reader.read_event_into(&mut buf) {
        Ok(event) => println!("{event:?}"),
        Err(e) => panic!("{e:?}"),
    }
    println!("buffered {} {:?}", buf.len(), buf);
    buf.clear();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Text(e)) => println!("text {}", e.decode().unwrap()),
            Ok(other) => println!("{other:?}"),
            Err(e) => panic!("{e:?}"),
        }
        println!("after {}", buf.len());
        buf.clear();
    }
    println!("position {}", reader.buffer_position());
    let mut again = Reader::from_str("<c>two</c>");
    loop {
        match again.read_event() {
            Ok(Event::Eof) => break,
            Ok(event) => println!("read {event:?}"),
            Err(e) => panic!("{e:?}"),
        }
    }
}
