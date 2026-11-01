// rawr
// A crate's async functions that borrow their arguments (D683.2): each
// future holds the font it borrows, shared or mutably, across its await,
// and the font is whole again after it. `cargo run` prints the same.

#[tokio::main]
async fn main() {
    let mut font = shelf::font(String::from("Georgia"), 3);
    let size = shelf::load(&font).await;
    shelf::rename(&mut font, String::from("Inter")).await;
    let again = shelf::load(&font).await;
    println!("{} {size} {again}", font.family);
}
