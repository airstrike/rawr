fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    r.push(3);
    r = &mut b;
    let b = 5;
    r.push(b);
    let a = "shadowed";
    r.push(7);
    println!("{a} {b} {r:?}");
}
