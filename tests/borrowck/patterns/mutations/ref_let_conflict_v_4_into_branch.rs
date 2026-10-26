fn main() {
    let mut v = vec![1];
    let ref a = v;
    let __b = &mut v;
    v.push(1);
    if __c() {
        __touch(__b);
    }
    println!("{:?}", a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
