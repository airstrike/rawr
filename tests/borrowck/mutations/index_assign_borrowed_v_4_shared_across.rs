fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __s = &v;
    v[0] = 5;
    __see(__s);
    println!("{}", r[0]);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
