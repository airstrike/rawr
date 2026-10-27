fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __g = || __see(&v);
    v.push(2);
    __g();
    println!("{}", s.len());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
