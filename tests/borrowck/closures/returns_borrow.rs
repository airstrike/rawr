fn main() {
    let mut v = vec![1, 2];
    let c = || &v;
    let r = c();
    v.push(3);
    println!("{:?}", r);
}
