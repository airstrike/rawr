fn main() {
    let mut v = vec![1, 2];
    let __s = &v;
    v.push(v.len());
    __see(__s);
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
