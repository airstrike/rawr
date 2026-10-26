fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    let __s = &v;
    v.push(1);
    __see(__s);
    a.push(2);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
