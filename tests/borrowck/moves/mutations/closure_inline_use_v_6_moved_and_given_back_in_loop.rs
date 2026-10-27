fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    v.push(doubled.len() as i64);
    for __i in 0..2 {
        let __m = v;
        v = __m;
    }
    println!("{:?} {:?}", v, doubled);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
