fn main() {
    let mut b = vec![0];
    let r = &mut b;
    let b = 9;
    let mut f = |n: i64| r.push(n);
    f(b);
    println!("{b}");
}
