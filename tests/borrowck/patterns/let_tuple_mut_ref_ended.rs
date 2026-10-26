fn main() {
    let mut t = (vec![1], vec![2]);
    let (a, _) = &mut t;
    a.push(4);
    t.1.push(3);
    println!("{:?}", t);
}
