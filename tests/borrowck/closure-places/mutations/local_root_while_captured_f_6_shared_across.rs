fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut f = || r.push(2);
    v.push(3);
    let __s = &f;
    f();
    __see(__s);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
