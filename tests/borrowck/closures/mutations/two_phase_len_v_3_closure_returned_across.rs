fn main() {
    let mut v = vec![1, 2];
    let __h = || &v;
    let __k = __h();
    v.push(v.len());
    __see(__k);
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
