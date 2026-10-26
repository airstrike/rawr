fn main() {
    let mut v = vec![1];
    let ref a = v;
    println!("{:?}", a);
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        v.push(1);
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
