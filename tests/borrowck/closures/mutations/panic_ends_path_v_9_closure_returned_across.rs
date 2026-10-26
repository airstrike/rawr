fn main() {
    let mut v = vec![1];
    let r = &mut v;
    if v.is_empty() {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
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
