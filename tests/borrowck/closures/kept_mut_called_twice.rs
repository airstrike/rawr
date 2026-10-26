fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(3);
    c();
    c();
    println!("{}", v.len());
    println!("{:?}", v);
}
