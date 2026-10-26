fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    r.push(5);
    r = &mut b;
    b.push(1);
    r.push(2);
    println!("{a:?} {b:?}");
}
