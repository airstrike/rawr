fn main() {
    let a = 3;
    let mut o = Some(&&a);
    let x = o.copied();
    o.take();
    println!("{x:?} {o:?}");
}
