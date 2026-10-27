fn main() {
    let a = 1;
    let mut o = Some(&a);
    let x = o.as_ref().copied().unwrap();
    o = None;
    println!("{x} {o:?}");
}
