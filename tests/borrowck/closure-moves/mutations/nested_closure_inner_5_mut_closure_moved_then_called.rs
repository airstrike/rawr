fn main() {
    let mut v = vec![1, 2];
    let mut outer = || {
        let mut inner = || v.push(1);
        let mut __f = || __touch(&mut inner);
        let mut __g = __f;
        __g();
        __f();
        inner();
    };
    v.len();
    outer();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
