fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(1);
    let __g = || __see(&c);
    c();
    __g();
    v.len();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
