fn main() {
    let mut v = vec![1];
    let r = &v;
    let __b = &mut v;
    __touch(__b);
    v.push(1);
    println!("{r:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
