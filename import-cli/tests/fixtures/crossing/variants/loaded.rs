// rawr
// fount's loading program: `Task::future` of an async block that awaits a
// crate's `async fn`, and a closure `.then` takes the block's output, which
// it matches on. iced's `Task::future` takes `impl Future<Output = T>`, so
// the closure's input is that output, a `Result` holding the crate's error
// (#652). An async block may call std's functions by their full path
// (#659).

use events::fonts::{self, Font};
use events::Task;

#[derive(Debug)]
enum Message {
    Loaded(usize),
    Failed(String),
    Read(bool),
}

fn load(family: &str) -> Task<Message> {
    let font = Font {
        family: family.to_owned(),
    };
    Task::future(async move { fonts::load(&font).await }).then(move |bytes| match bytes {
        Ok(bytes) => Task::done(Message::Loaded(bytes.len())),
        Err(error) => Task::done(Message::Failed(error.to_string())),
    })
}

fn main() {
    println!("{:?}", load("Georgia").value());
    println!("{:?}", load("").value());
    let path = String::from("Cargo.toml");
    let read = Task::future(async move { std::fs::read(path) })
        .then(|bytes| Task::done(Message::Read(bytes.is_ok_and(|bytes| !bytes.is_empty()))));
    println!("{:?}", read.value());
}
