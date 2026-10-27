fn main() {
    let mut v = vec![1, 2];
    let __t = vec![&v].first();
    v.push(v.len());
    __see(&__t);
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
