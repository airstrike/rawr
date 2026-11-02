// rawr
// A crate's async functions whose futures borrow std's types and Rust's
// own (#714): `&Path` and `&[u8]`, `&String`, `&Vec<u32>`, `&u32` and a
// `Result<&Font, String>`. Each borrow is leased to the future for its
// life, and the values are whole again after the await. `cargo run`
// prints the same.

use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let path = PathBuf::from("out/notes.txt");
    let data = vec![1u8, 2, 3];
    println!("{}", shelf::store::save(&path, &data).await);
    let text = String::from("quiet");
    println!("{}", shelf::store::shout(&text).await);
    let values = vec![1u32, 2, 3, 4];
    println!("{}", shelf::store::total(&values).await);
    let count = 21u32;
    println!("{}", shelf::store::doubled(&count).await);
    let font = shelf::font(String::from("Inter"), 2);
    println!("{}", shelf::store::family(Ok(&font)).await);
    println!(
        "{}",
        shelf::store::family(Err(String::from("missing"))).await
    );
    println!(
        "{} {text} {} {count} {}",
        path.display(),
        values.len(),
        font.family
    );
}
