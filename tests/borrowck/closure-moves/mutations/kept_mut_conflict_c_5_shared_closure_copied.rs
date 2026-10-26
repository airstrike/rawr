fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    v.len();
    let __f = || __see(&c);
    let __g = __f;
    __f();
    c();
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
