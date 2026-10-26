fn main() {
    let mut v = vec![1, 2];
    let c = || &v;
    let r = c();
    println!("{:?}", r);
    let __h = || &v;
    let __k = __h();
    v.push(3);
    __see(__k);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
