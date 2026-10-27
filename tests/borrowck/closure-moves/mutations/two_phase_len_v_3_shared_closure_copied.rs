fn main() {
    let mut v = vec![1, 2];
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(v.len());
    __g();
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
