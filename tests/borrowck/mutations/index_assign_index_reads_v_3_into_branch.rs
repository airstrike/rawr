fn main() {
    let mut v = vec![1, 2];
    let __b = &mut v;
    v[v.len() - 1] = 5;
    if __c() {
        __touch(__b);
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
