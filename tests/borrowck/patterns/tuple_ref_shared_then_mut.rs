fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref a, _) = t;
    t.0.push(1);
    println!("{:?}", a);
}
