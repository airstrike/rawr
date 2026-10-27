fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    r.push(3);
    f();
    println!("{v:?}");
}
