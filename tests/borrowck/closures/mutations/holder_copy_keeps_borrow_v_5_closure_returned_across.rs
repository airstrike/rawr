fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __h = || &v;
    let __k = __h();
    v.push(2);
    __see(__k);
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
