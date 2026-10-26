fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    v[0] = 5;
    println!("{}", r[0]);
}
