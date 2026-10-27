fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    s.push(2);
    r.push(3);
    if __c() {
        __take(v);
        panic!();
    }
    println!("{:?}", v);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
