fn main() {
    let mut v = vec![1];
    let r = &v;
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(1);
    __g();
    println!("{r:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
