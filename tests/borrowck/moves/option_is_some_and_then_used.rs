fn main() {
    let o = Some(String::from("ab"));
    let b = o.is_some_and(|s| s.len() > 1);
    println!("{} {:?}", b, o);
}
