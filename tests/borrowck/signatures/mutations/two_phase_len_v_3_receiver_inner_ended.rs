fn main() {
    let mut v = vec![1, 2];
    let __t = vec![&v].to_vec();
    __see(&__t);
    v.push(v.len());
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
