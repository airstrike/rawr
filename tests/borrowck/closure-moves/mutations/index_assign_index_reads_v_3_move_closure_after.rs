fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let __m = move || __see(&v);
    __m();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
