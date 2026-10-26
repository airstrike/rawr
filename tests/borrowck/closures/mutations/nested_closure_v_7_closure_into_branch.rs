fn main() {
    let mut v = vec![1, 2];
    let mut outer = || {
        let mut inner = || v.push(1);
        inner();
    };
    let mut __f = || __touch(&mut v);
    v.len();
    if __c() {
        __f();
    }
    outer();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
