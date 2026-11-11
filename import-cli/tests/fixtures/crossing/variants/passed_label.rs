// rawr
// A crate future's output that holds a `&'static str`, `(u32, &str)` from
// `passed_beside(.., "beside")`, reaches a script closure as its input, as
// rustc allows: the closure takes the tuple apart and keeps the string.

use events::Task;

#[derive(Debug)]
enum Message {
    Waited(u32, &'static str),
}

fn wait(ticks: u32) -> Task<Message> {
    Task::perform(
        events::passed_beside(async move { ticks * 3 }, "beside"),
        move |(waited, beside)| Message::Waited(waited, beside),
    )
}

fn main() {
    println!("{:?}", wait(5).value());
}
