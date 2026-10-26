fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __s = &v;
    r.push(v.len());
    __see(__s);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
