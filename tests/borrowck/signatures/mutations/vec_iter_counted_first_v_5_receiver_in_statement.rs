fn main() {
    let mut v = vec![1, 2];
    let n = v.iter().count();
    v.push(3);
    __see(&vec![&v].first());
    println!("{n} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
