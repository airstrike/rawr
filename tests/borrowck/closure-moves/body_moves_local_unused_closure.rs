fn main() {
    let x = String::from("ab");
    || x;
    let y = &x;
    println!("{}", y);
}
