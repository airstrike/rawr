fn main() {
    let t = (String::from("ab"), String::from("cde"));
    let c = move || t.0.len();
    let b = t.1;
    println!("{} {}", c(), b);
}
