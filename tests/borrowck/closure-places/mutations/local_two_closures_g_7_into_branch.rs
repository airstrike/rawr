fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    let mut g = || r.push(3);
    f();
    let __b = &mut g;
    g();
    if __c() {
        __touch(__b);
    }
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
