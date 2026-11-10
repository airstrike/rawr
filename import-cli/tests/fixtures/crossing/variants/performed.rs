// rawr
// iced's toast: `Task::perform(tokio::time::sleep(..), move |_|
// Message::Close(id))`, where `Task::perform` takes a future and a closure
// over its output. A crate's own future fills it, and so does an async
// block, each beside a closure that captures `id` (#1073).

use events::Task;

#[derive(Debug)]
enum Message {
    Close(u64),
    Waited(u64, u32),
}

fn expire(id: u64, ticks: u32) -> Task<Message> {
    Task::perform(events::sleep(ticks), move |_| Message::Close(id))
}

fn wait(id: u64, ticks: u32) -> Task<Message> {
    Task::perform(async move { ticks * 2 }, move |waited| {
        Message::Waited(id, waited)
    })
}

fn main() {
    println!("{:?}", expire(7, 3).value());
    println!("{:?}", wait(8, 5).value());
}
