fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __h = || &v;
    let __k = __h();
    v.push(r.len());
    __see(__k);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
