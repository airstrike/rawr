fn main() {
    let o = Some(String::from("ab"));
    let s = o.unwrap();
    println!("{} {:?}", s, o);
}
