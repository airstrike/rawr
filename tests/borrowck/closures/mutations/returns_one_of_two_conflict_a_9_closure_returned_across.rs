fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let c = || {
        b.len();
        &a
    };
    let r = c();
    let __h = || &a;
    let __k = __h();
    a.push(3);
    __see(__k);
    println!("{:?} {:?}", r, b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
