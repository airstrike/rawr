fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    let __t = vec![&v].first();
    v.push(3);
    __see(&__t);
    println!("{n} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
