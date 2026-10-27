fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    println!("{}", c());
    v.push(3);
    println!("{:?}", v);
}
