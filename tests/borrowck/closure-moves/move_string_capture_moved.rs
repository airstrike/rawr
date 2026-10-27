fn main() {
    let s = String::from("abc");
    let c = move || s.len();
    let d = c;
    println!("{} {}", d(), c());
}
