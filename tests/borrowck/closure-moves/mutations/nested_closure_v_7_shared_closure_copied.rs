fn main() {
    let mut v = vec![1, 2];
    let mut outer = || {
        let mut inner = || v.push(1);
        inner();
    };
    let __f = || __see(&v);
    let __g = __f;
    __f();
    v.len();
    __g();
    outer();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
