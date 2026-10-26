fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    let __b = &mut v;
    v.push(1);
    if __c() {
        __touch(__b);
    }
    a.push(2);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
