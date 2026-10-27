fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    let mut g = || r.push(3);
    f();
    let __b = &mut g;
    __touch(__b);
    g();
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
