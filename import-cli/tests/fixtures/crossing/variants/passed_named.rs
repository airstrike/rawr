// rawr
// Crate functions whose own generics are named as Rawr's synthetic
// parameters could be: `SOutput` beside a future parameter `S`, and
// `impl_1` beside an argument's `impl Trait`. Rust allows both names.

use events::Task;

#[derive(Debug)]
enum Message {
    Waited(u32, u64),
}

fn wait(ticks: u32) -> Task<Message> {
    Task::perform(
        events::passed_beside(async move { ticks * 3 }, 7_u64),
        move |(waited, beside)| Message::Waited(waited, beside),
    )
}

fn main() {
    println!("{:?}", wait(5).value());
    println!("{}", events::summed(2_u8, 5_u16));
}
