fn main() {
    let x = String::from("ab");
    let c = || x;
    let y = x;
    println!("{} {}", c(), y);
}
