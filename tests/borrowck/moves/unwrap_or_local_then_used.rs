fn main() {
    let o = Some(String::from("ab"));
    let s = o.unwrap_or(String::new());
    println!("{} {:?}", s, o);
}
