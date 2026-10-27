fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    f();
    f();
    let __b = &mut v;
    println!("{v:?}");
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
