fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let __g = || __see(&v);
    v.push(3);
    __g();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
