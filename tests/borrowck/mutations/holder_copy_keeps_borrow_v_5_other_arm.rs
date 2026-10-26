fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        v.push(2);
    }
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
