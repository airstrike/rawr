fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    println!("{}", v[1]);
    let __h = || &c;
    let __k = __h();
    c();
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
