// rawr
// iced's `Task::sip` takes `impl FnOnce(<S as Future>::Output) -> T`: an
// associated type of its own type parameter, which the type the call fixes
// `S` to decides (#568).

mod download;

use events::Task;

#[derive(Debug)]
enum Update {
    Finished(u32),
}

fn main() {
    let task: Task<Update> = Task::finish(download::finished("https://x"), Update::Finished);
    println!("{:?}", task.value());
}
