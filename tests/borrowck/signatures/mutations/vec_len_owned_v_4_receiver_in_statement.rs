fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    __see(&vec![&v].first());
    v.push(3);
    println!("{n} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
