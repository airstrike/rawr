fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(2);
    __g();
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
