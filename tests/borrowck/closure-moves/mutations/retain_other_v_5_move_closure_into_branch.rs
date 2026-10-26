fn main() {
    let mut v = vec![1, 2, 3];
    let limit = vec![2];
    v.retain(|x| *x < limit[0]);
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
