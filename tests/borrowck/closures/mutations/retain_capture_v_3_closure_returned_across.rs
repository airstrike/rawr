fn main() {
    let mut v = vec![1, 2, 3];
    let __h = || &v;
    let __k = __h();
    v.retain(|x| *x < v.len() as i32);
    __see(__k);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
