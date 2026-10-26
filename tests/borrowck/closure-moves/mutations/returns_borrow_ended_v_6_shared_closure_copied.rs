fn main() {
    let mut v = vec![1, 2];
    let c = || &v;
    let r = c();
    println!("{:?}", r);
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.push(3);
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
