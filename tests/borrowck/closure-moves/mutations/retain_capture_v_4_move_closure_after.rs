fn main() {
    let mut v = vec![1, 2, 3];
    v.retain(|x| *x < v.len() as i32);
    println!("{:?}", v);
    let __m = move || __see(&v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
