fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v[0] = 5;
    __g();
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
