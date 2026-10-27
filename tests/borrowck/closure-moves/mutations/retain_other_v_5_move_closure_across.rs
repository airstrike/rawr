fn main() {
    let mut v = vec![1, 2, 3];
    let limit = vec![2];
    v.retain(|x| *x < limit[0]);
    let __m = move || __see(&v);
    println!("{:?}", v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
