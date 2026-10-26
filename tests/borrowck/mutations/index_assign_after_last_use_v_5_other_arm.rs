fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        v[0] = 5;
    }
    println!("{} {:?}", first, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
