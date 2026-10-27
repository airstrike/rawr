fn main() {
    let t = (String::from("ab"), String::from("cde"));
    let b = t.1;
    let r = &t.0;
    println!("{} {}", r, b);
}
