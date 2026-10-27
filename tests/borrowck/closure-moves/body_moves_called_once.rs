fn main() {
    let x = String::from("ab");
    let c = || x;
    let a = c();
    println!("{}", a);
}
