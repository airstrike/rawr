fn main() {
    let mut v = vec![1, 2];
    let c = || v.len() + 1;
    let n = c();
    v.push(3);
    let __h = || &v;
    let __k = __h();
    println!("{} {:?}", n, v);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
