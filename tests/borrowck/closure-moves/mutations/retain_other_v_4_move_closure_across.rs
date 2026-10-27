fn main() {
    let mut v = vec![1, 2, 3];
    let limit = vec![2];
    let __m = move || __see(&v);
    v.retain(|x| *x < limit[0]);
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
