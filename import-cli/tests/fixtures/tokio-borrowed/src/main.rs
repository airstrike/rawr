// rawr
// A crate's async functions that borrow their arguments (D683.2): each
// future holds the font it borrows, shared or mutably, across its await,
// and the font is whole again after it. A future borrows through any input
// type, as `&str`, `Option<&Catalog>` and a tuple of borrows (#677). `cargo run` prints the
// same.

#[tokio::main]
async fn main() {
    let mut font = shelf::font(String::from("Georgia"), 3);
    let size = shelf::load(&font).await;
    shelf::rename(&mut font, String::from("Inter")).await;
    let again = shelf::load(&font).await;
    println!("{} {size} {again}", font.family);

    let catalog = shelf::google::catalog(vec![String::from("Inter")]);
    let family = font.family.clone();
    let listed = async move { shelf::google::load(&family, Some(&catalog)).await }.await;
    let bare = async move { shelf::google::load("Georgia", None).await }.await;
    let kept = Some(shelf::google::catalog(vec![String::from("Aptos")]));
    let name = String::from("Inter");
    let unlisted = async move { shelf::google::load(&name, kept.as_ref()).await }.await;
    let other = shelf::google::catalog(vec![String::from("Aptos"), String::from("Inter")]);
    let paired = async move { shelf::google::pair(("Georgia", &other)).await }.await;
    println!("{listed}; {bare}; {unlisted}; {paired}");
}
