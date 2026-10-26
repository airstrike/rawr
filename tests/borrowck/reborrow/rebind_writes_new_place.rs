fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    r = &mut b;
    r.push(3);
    println!("{a:?} {b:?}");
}
