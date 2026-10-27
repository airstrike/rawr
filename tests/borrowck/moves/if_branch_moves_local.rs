fn main() {
    let s = String::from("ab");
    let t = if s.len() > 1 { s } else { String::new() };
    println!("{}", t);
}
