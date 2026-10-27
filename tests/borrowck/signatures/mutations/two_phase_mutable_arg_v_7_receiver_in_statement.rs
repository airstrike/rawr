fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    __see(&vec![&v].first());
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
