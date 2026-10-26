fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __s = &v;
    v.push(2);
    __see(__s);
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
