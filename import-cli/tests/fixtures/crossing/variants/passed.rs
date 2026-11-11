// rawr
// A crate function whose output future's `Output` is its future
// parameter's `S::Output`, as futures' adapters keep their input's output.
// The parameter's synthetic `SOutput` is its future's output, named once.

use events::Task;

#[derive(Debug)]
enum Message {
    Waited(u64, u32),
}

fn wait(id: u64, ticks: u32) -> Task<Message> {
    Task::perform(events::passed(async move { ticks * 3 }), move |waited| {
        Message::Waited(id, waited)
    })
}

fn main() {
    println!("{:?}", wait(4, 5).value());
}
