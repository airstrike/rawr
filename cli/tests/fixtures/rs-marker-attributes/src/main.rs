// rawr
use rawr::preview;

mod label;

#[preview]
fn greeting() -> String {
    String::from("hello")
}

fn seeded() -> String {
    String::from("seeded")
}

#[preview(name = "seeded", seed = seeded)]
fn shown(text: String) -> String {
    text
}

fn main() {
    println!("{} {}", greeting(), label::label(3));
}
