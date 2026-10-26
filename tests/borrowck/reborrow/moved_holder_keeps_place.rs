fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    let s = r;
    r = &mut b;
    s.push(5);
    r.push(6);
    println!("{a:?} {b:?}");
}
