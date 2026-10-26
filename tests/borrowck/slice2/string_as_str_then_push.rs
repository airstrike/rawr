fn main() {
    let mut s = String::from("hi");
    let t = s.as_str();
    s.push_str("!");
    println!("{t}");
}
