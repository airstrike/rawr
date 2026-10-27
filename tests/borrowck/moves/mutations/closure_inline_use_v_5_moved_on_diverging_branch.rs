fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    if __c() {
        __take(v);
        panic!();
    }
    v.push(doubled.len() as i64);
    println!("{:?} {:?}", v, doubled);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
