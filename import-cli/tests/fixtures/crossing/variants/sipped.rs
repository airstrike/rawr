// rawr
// iced's `Task::sip`: its closure takes the sipper's own `Item`, which the
// type the call fixes `S` to decides (#568).

mod download;

use events::Task;

#[derive(Debug)]
enum Update {
    Downloading(u32),
}

fn main() {
    let task: Task<Update> = Task::sip(download::sizes("https://x"), Update::Downloading);
    println!("{:?}", task.value());
}
