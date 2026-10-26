fn main() {
    let mut v = vec![1];
    let a = &mut v;
    let b = &mut v;
    b.push(2);
    a.push(3);
    println!("{:?}", v);
}
