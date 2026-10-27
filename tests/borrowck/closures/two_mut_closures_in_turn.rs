fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    a();
    let mut b = || v.push(2);
    b();
    println!("{:?}", v);
}
