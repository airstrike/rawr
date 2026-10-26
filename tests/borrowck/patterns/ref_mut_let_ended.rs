fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    a.push(2);
    v.push(1);
    println!("{:?}", v);
}
