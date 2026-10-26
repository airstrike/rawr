fn add(v: &mut Vec<i64>, n: i64) {
    v.push(n);
}
fn main() {
    let mut b = vec![0];
    let r = &mut b;
    let b = vec![7];
    add(r, 1);
    *r = vec![2, 3];
    add(r, b[0]);
    println!("{b:?}");
    let total: i64 = r.iter().sum();
    println!("{total}");
}
