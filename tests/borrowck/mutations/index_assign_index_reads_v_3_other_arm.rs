fn main() {
    let mut v = vec![1, 2];
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        v[v.len() - 1] = 5;
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
