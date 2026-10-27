fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    s.push(2);
    r.push(3);
    let __m = v;
    v = __m;
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
