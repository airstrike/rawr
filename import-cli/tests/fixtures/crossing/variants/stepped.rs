// rawr
// iced's download_progress: the program's own native `download(url: impl
// AsRef<str>) -> impl Straw<..>`, whose opaque type captures the `impl
// AsRef<str>` input, fills a crate function's generic `S`, and a variant
// takes the work's last step (#568).

mod download;

use events::Task;

#[derive(Debug)]
enum Update {
    Finished(u32),
}

fn main() {
    let task: Task<Update> = Task::stepped(download::download("https://x"), Update::Finished);
    println!("{:?}", task.value());
    let none: Task<Update> = Task::stepped(download::download(String::new()), Update::Finished);
    println!("{:?}", none.value());
}
