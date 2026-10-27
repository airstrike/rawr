fn main() {
    let mut v = vec![1, 2];
    let mut outer = || {
        let mut inner = || v.push(1);
        inner();
        let __m = move || __see(&inner);
        __m();
    };
    v.len();
    outer();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
