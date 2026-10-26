fn main() {
    let mut v = vec![1];
    let r = &v;
    let __h = || &v;
    let __k = __h();
    v.push(1);
    __see(__k);
    println!("{r:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
