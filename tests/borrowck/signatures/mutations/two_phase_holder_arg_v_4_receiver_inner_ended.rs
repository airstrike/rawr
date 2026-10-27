fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __t = vec![&v].to_vec();
    __see(&__t);
    r.push(v.len());
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
