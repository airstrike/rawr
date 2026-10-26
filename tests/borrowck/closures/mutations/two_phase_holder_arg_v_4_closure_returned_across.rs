fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __h = || &v;
    let __k = __h();
    r.push(v.len());
    __see(__k);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
