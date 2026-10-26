fn take(v: Vec<i64>) -> usize {
    v.len()
}
fn main() {
    let v = vec![1];
    let r = &v;
    let m = r.len();
    let n = take(v);
    println!("{} {}", n, m);
}
