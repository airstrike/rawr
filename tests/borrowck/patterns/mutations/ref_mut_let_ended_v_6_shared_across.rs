fn main() {
    let mut v = vec![1];
    let ref mut a = v;
    a.push(2);
    v.push(1);
    let __s = &v;
    println!("{:?}", v);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
