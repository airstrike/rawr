fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.and_then(|_| Some(&b));
    b += 1;
    println!("{r:?}");
}
