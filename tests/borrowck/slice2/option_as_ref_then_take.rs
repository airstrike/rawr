fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    o.take();
    println!("{r:?}");
}
