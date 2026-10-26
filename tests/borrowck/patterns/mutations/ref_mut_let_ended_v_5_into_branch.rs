fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    a.push(2);
    let __b = &mut v;
    v.push(1);
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
