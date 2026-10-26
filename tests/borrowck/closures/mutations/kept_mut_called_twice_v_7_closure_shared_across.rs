fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(3);
    c();
    c();
    println!("{}", v.len());
    let __g = || __see(&v);
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
