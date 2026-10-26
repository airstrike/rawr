fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    v.len();
    c();
    println!("{:?}", v);
}
