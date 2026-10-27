fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    f();
    f();
    println!("{v:?}");
}
