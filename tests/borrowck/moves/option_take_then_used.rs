fn main() {
    let mut o = Some(String::from("ab"));
    let s = o.take();
    println!("{:?} {:?}", s, o);
}
