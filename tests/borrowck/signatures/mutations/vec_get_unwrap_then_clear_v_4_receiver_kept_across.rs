fn main() {
    let mut v = vec![10, 20];
    let x = v.get(1).unwrap();
    let __t = vec![&v].first();
    v.clear();
    __see(&__t);
    println!("{x}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
