fn main() {
    let mut v = vec![1];
    let ref a = v;
    println!("{:?}", a);
    v.push(1);
    println!("{:?}", v);
}
