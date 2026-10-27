fn main() {
    let t = (String::from("ab"), String::from("cde"));
    let (a, _) = t;
    println!("{} {}", a, t.1);
}
