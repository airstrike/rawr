fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    let __h = || &v;
    let __k = __h();
    v.push(1);
    __see(__k);
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
