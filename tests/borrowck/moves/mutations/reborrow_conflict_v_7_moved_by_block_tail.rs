fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    r.push(3);
    s.push(2);
    let __m = { v };
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
