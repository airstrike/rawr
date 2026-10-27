fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.map(|_| &b);
    b += 1;
    println!("{r:?}");
}
