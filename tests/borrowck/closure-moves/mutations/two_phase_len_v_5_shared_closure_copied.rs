fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(r.len());
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
