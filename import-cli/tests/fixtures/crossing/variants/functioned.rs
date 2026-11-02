// rawr
// iced's gallery: `Message::BlurhashDecoded.with(id)` is the crate's own
// `Function::with`, which its blanket impl over `Fn(A, B) -> O` gives a
// variant's constructor, a plain function and a closure. Each returns
// `impl Fn(B) -> O`, which crate functions take as their callables.

mod download;

use download::{Bytes, Image, Size};
use events::{Function, Task};

#[derive(Debug)]
enum Message {
    BlurhashDecoded(u32, u32),
    ThumbnailDownloaded(u32, Result<Bytes, String>),
    Tagged(u32, u32),
}

fn tagged(id: u32, value: u32) -> Message {
    Message::Tagged(id * 10, value)
}

fn main() {
    let id = 4;
    let image = Image {
        id,
        url: "image".to_owned(),
    };
    let task: Task<Message> = Task::finish(
        image.download(Size::Thumbnail { width: 7 }),
        Message::BlurhashDecoded.with(id),
        Message::ThumbnailDownloaded.with(id),
    );
    println!("{:?}", task.value());
    let plain = Task::done(9).map(tagged.with(id + 1));
    println!("{:?}", plain.value());
    let offset = 100;
    let closure =
        Task::done(2).map((move |a: u32, b: u32| Message::Tagged(a + offset, b)).with(id));
    println!("{:?}", closure.value());
}
