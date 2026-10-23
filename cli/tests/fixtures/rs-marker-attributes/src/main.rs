// rawr
use rawr::preview;

mod label;

#[preview]
fn greeting() -> String {
    String::from("hello")
}

fn main() {
    println!("{} {}", greeting(), label::label(3));
}
