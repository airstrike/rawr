fn main() {
    let mut v = vec![1, 2];
    let s = &v;
    let c = || &s[0];
    let r = c();
    v.push(3);
    println!("{}", r);
}
