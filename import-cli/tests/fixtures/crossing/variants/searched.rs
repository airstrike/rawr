// rawr
use events::handed::{self, Bytes};

#[derive(Debug)]
struct Found {
    bytes: Bytes,
}

#[derive(Debug)]
enum Fault {
    Empty,
}

#[derive(Debug)]
enum Message {
    Searched(Result<Found, Fault>),
}

async fn search(size: u8) -> Result<Found, Fault> {
    if size == 0 {
        return Err(Fault::Empty);
    }
    let values = (1..=size).collect::<Vec<u8>>();
    Ok(Found {
        bytes: Bytes::new(&values),
    })
}

fn main() {
    for size in [3, 0, 5] {
        let Message::Searched(searched) = handed::perform(search(size), Message::Searched);
        match searched {
            Ok(found) => println!("found {:?}", found.bytes.as_slice()),
            Err(fault) => println!("fault {fault:?}"),
        }
    }
}
