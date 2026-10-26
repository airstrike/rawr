fn main() {
    let mut v = vec![1];
    let mut a = || v.push(1);
    let mut b = || v.push(2);
    a();
    let mut __f = || __touch(&mut b);
    for __i in 0..2 {
        __f();
        b();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
