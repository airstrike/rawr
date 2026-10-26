fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __b = &mut v;
    v[0] = 5;
    if __c() {
        __touch(__b);
    }
    println!("{} {:?}", first, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
