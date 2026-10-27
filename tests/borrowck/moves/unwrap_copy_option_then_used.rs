fn main() {
    let o = Some(3);
    let n = o.unwrap();
    println!("{} {:?}", n, o);
}
