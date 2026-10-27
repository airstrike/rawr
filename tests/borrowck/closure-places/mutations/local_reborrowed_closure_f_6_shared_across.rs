fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &*r;
    let mut f = || r.push(2);
    let __s = &f;
    f();
    __see(__s);
    println!("{s:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
