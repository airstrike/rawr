// rawr
// iced's `Task::sip` takes `impl FnOnce(<S as Future>::Output) -> T`: a std
// trait's associated type of its own type parameter, which the type the call
// fixes `S` to decides through `Straw`, `Sipper` and `Core` (#621).

mod download;

use events::Task;

#[derive(Debug)]
enum Update {
    Downloading(u32),
    Finished(Result<u32, String>),
}

fn main() {
    let task: Task<Update> = Task::finish(
        download::finished("https://x"),
        Update::Downloading,
        Update::Finished,
    );
    println!("{:?}", task.value());
}
