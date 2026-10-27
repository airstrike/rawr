fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    v[0] = 5;
    let __f = || __see(&v);
    let __g = __f;
    __f();
    println!("{} {:?}", first, v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
