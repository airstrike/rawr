fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, _) = t;
    t.0.push(3);
    a.push(4);
    println!("{:?}", t);
}
