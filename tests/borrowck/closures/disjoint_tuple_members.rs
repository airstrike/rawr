fn main() {
    let mut t = (vec![1], vec![2]);
    let mut c = || t.0.push(3);
    t.1.push(4);
    c();
    println!("{:?} {:?}", t.0, t.1);
}
