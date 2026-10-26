fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let __b = &mut v;
    println!("{:?}", v);
    __touch(__b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
