fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let d = || c() + 1;
    println!("{}", d());
    v.push(1);
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
