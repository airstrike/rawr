fn main() {
    let mut v = vec![1];
    let ref a = v;
    v.push(1);
    println!("{:?}", a);
}
