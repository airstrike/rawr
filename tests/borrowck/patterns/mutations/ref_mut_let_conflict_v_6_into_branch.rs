fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    v.push(1);
    a.push(2);
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
