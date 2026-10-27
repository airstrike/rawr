fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut r = &mut a;
    let __s = &r;
    r.push(10);
    __see(__s);
    r = &mut b;
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
