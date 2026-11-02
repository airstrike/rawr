// rawr
// iced's gallery: `Task::future(image.download(Size::Original))`, where the
// program's own native `Image::download(self, size)` gives `impl
// Straw<Bytes, u32, String>` and `Task::future` takes `impl Future<Output =
// T>`, so T is the `Result<Bytes, String>` that the opaque type's `Future`
// binds through `Straw`, `Sipper` and `Core`. `.and_then` and `.then` take it
// from there (#637).

mod download;

use download::{Bytes, Image, Size};
use events::Task;

#[derive(Debug)]
enum Message {
    Downloaded(Result<Bytes, String>),
}

fn open(image: Image, size: Size) -> Task<Message> {
    Task::future(image.download(size))
        .and_then(|bytes| Task::done(Ok(Bytes(bytes.0 + 1))))
        .then(|bytes| Task::done(Message::Downloaded(bytes)))
}

fn main() {
    let image = Image {
        id: 4,
        url: "https://x".to_owned(),
    };
    println!("{:?}", open(image.clone(), Size::Original).value());
    println!("{:?}", open(image, Size::Thumbnail { width: 0 }).value());
}
