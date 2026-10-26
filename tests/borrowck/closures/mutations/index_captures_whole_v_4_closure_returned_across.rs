fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    let __h = || &v;
    let __k = __h();
    println!("{}", v[1]);
    __see(__k);
    c();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
