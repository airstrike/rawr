fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    r.push(10);
    r = &mut b;
    let mut f = || r.push(20);
    f();
    f();
    let __b = &mut b;
    for __i in 0..2 {
        __touch(__b);
        println!("{a:?} {b:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
