fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    if b.len() > 0 {
        r = &mut b;
    }
    a.push(4);
    r.push(3);
    println!("{a:?} {b:?}");
}
