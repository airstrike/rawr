fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    let __h = || &b;
    let __k = __h();
    b.push(3);
    __see(__k);
    println!("{:?} {:?}", r, b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
