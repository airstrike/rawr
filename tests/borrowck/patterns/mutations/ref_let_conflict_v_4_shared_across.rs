fn main() {
    let mut v = vec![1];
    let ref a = v;
    let __s = &v;
    v.push(1);
    __see(__s);
    println!("{:?}", a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
