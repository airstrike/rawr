fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    a.push(3);
    println!("{:?} {:?}", r, b);
}
