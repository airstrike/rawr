fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    b.push(3);
    println!("{:?} {:?}", r, b);
}
