fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    let mut g = || r.push(3);
    let __b = &mut f;
    for __i in 0..2 {
        __touch(__b);
        f();
    }
    g();
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
