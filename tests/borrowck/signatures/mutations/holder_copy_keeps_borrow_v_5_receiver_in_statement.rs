fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    __see(&vec![&v].first());
    v.push(2);
    println!("{}", s.len());
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
