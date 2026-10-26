fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    c();
    v.len();
    println!("{:?}", v);
}
