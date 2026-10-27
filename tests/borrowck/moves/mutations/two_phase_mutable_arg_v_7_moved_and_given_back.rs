fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    let __m = v;
    v = __m;
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
