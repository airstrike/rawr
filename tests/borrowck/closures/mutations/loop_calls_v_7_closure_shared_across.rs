fn main() {
    let mut v = vec![1];
    let mut c = || v.push(1);
    for _ in 0..3 {
        c();
    }
    let __g = || __see(&v);
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
