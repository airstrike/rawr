fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    println!("{}", c());
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(1);
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
