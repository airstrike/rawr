fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    let __g = || __see(&v);
    println!("{}", v[1]);
    __g();
    c();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
