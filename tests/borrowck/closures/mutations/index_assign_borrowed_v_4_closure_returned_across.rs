fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __h = || &v;
    let __k = __h();
    v[0] = 5;
    __see(__k);
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
