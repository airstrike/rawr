fn main() {
    let t = (String::from("ab"), String::from("cde"));
    let b = t.1;
    let u = t;
    println!("{} {}", u.0, b);
}
