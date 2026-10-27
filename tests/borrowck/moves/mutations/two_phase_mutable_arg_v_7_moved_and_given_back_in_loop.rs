fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    for __i in 0..2 {
        let __m = v;
        v = __m;
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
