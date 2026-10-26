fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let kept = &mut a;
    kept.push(3);
    println!("{kept:?}");
    let mut r = &mut b;
    r.push(4);
    println!("{r:?}");
    r = &mut a;
    r.push(5);
    println!("{r:?} {b:?}");
}
