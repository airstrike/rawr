fn main() {
    let x = String::from("ab");
    let c = || x;
    let a = c();
    let b = c();
    println!("{} {}", a, b);
}
