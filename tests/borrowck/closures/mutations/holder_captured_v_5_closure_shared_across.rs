fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    let __g = || __see(&v);
    v.push(1);
    __g();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
