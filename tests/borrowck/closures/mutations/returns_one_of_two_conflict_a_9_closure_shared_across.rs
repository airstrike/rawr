fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    let __g = || __see(&a);
    a.push(3);
    __g();
    println!("{:?} {:?}", r, b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
