fn main() {
    let mut v = vec![1, 2];
    let c = || v.len() + 1;
    let n = c();
    let __h = || &v;
    let __k = __h();
    v.push(3);
    __see(__k);
    println!("{} {:?}", n, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
