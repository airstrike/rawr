fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    __see(&vec![&v].first());
    v.push(3);
    println!("{w:?} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
