fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.unwrap();
    if s.len() > 1 {
        o = Some(String::from("c"));
    }
    println!("{} {:?}", s, o);
}
