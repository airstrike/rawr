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

fn plain() -> u8 {
    0
}

fn styled(_: &str) -> u8 {
    1
}

#[preview(name = "seeded", seed = seeded, themes = [plain, styled])]
fn shown(text: String) -> String {
    text
}

fn main() {
    println!("{} {}", greeting(), label::label(3));
}
