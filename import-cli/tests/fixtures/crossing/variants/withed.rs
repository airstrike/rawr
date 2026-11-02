// rawr
// iced's gallery: `Task::sip(image.download(..),
// Message::BlurhashDecoded.with(id), Message::ThumbnailDownloaded.with(id))`,
// where each `.with(id)` is a closure that captures `id`. An item of
// several callable parameters takes a capturing closure in each, by that
// parameter's own call, also beside a slot, as rustc does. A closure input
// that holds a native value (`Result<Bytes, String>`) crosses through the
// arena (#728).

mod download;

use download::{Bytes, Image, Size};
use events::Task;

#[derive(Debug)]
enum Message {
    BlurhashDecoded(u32, u32),
    ThumbnailDownloaded(u32, Result<Bytes, String>),
    ThumbnailAllocated(u32, u32),
    Failed(Result<Bytes, String>),
}

fn main() {
    let id = 4;
    let image = Image {
        id,
        url: "image".to_owned(),
    };
    let task: Task<Message> = Task::finish(
        image.clone().download(Size::Thumbnail { width: 7 }),
        move |progress| Message::BlurhashDecoded(id, progress),
        move |bytes| Message::ThumbnailDownloaded(id, bytes),
    );
    println!("{:?}", task.value());
    let mixed: Task<Message> = Task::finish(
        image.download(Size::Thumbnail { width: 0 }),
        move |progress| Message::BlurhashDecoded(id, progress),
        Message::Failed,
    );
    println!("{:?}", mixed.value());
    let allocated = Task::done(9).map(move |x| Message::ThumbnailAllocated(id, x));
    println!("{:?}", allocated.value());
}
