fn main() {
    let o = Some(String::from("ab"));
    let s = o.as_ref().unwrap();
    println!("{} {:?}", s, o);
}
