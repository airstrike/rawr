fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    for n in 0..4 {
        let mut f = || r.push(n);
        let __b = &mut f;
        for __i in 0..2 {
            __touch(__b);
            f();
        }
        if n == 1 {
            r = &mut b;
        }
    }
    println!("{a:?} {b:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
