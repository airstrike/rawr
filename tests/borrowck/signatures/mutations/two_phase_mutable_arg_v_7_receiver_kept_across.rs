fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    let __t = vec![&v].first();
    println!("{:?}", v);
    __see(&__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
