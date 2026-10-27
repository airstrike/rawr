fn main() {
    let mut v = vec![1, 2];
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v[v.len() - 1] = 5;
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
