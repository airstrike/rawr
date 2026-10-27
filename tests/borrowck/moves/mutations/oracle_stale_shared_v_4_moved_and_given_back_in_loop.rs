fn main() {
    let mut v = vec![1];
    let r = &v;
    for __i in 0..2 {
        let __m = v;
        v = __m;
    }
    v.push(1);
    println!("{r:?}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
