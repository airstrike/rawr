fn main() {
    let t = (String::from("ab"), String::from("cde"));
    let r = &t.1;
    let b = t.1;
    println!("{} {}", r, b);
}
