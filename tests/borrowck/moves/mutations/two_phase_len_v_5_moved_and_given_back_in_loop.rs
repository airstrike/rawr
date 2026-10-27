fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    for __i in 0..2 {
        let __m = v;
        v = __m;
    }
    v.push(r.len());
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
