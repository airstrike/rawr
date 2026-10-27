fn main() {
    let t = (String::from("ab"), String::from("cde"));
    let c = move || t.0.len();
    println!("{} {}", c(), t.0);
}
