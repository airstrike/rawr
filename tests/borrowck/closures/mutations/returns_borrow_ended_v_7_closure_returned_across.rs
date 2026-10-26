fn main() {
    let mut v = vec![1, 2];
    let c = || &v;
    let r = c();
    println!("{:?}", r);
    v.push(3);
    let __h = || &v;
    let __k = __h();
    println!("{:?}", v);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
