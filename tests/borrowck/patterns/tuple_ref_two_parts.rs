fn main() {
    let mut t = (vec![1], vec![2]);
    let (ref mut a, ref b) = t;
    a.push(b.len() as i64);
    println!("{:?}", t);
}
