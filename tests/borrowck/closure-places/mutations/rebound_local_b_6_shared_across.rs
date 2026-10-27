fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    r.push(10);
    let __s = &b;
    r = &mut b;
    __see(__s);
    let mut f = || r.push(20);
    f();
    f();
    println!("{a:?} {b:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
