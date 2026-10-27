fn main() {
    let mut v = vec![1];
    let r = &v;
    __see(&vec![&v].first());
    v.push(1);
    println!("{r:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
