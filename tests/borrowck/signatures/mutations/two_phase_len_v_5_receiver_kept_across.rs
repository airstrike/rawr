fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __t = vec![&v].first();
    v.push(r.len());
    __see(&__t);
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
