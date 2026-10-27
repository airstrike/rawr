fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &*r;
    let mut f = || r.push(2);
    f();
    println!("{s:?}");
}
