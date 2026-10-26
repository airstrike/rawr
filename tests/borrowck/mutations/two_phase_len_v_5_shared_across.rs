fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __s = &v;
    v.push(r.len());
    __see(__s);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
