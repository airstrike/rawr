fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    f();
    f();
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
