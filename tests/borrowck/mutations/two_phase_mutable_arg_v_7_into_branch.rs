fn main() {
    let mut v = vec![1, 2];
    v.push({
        v.clear();
        3
    });
    let __b = &mut v;
    println!("{:?}", v);
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
