fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    __see(&vec![&v].first());
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
