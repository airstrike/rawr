fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __b = &mut v;
    __touch(__b);
    v[0] = 5;
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
