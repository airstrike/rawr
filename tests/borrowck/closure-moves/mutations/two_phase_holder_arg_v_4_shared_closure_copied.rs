fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __f = || __see(&v);
    let __g = __f;
    __f();
    r.push(v.len());
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
