fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let d = || c() + 1;
    println!("{}", d());
    v.push(1);
    println!("{:?}", v);
}
