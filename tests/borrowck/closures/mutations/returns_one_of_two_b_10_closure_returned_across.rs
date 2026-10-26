fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    b.push(3);
    let __h = || &b;
    let __k = __h();
    println!("{:?} {:?}", r, b);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
