fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    v.push(1);
    a.push(2);
    println!("{:?}", v);
}
