fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    v.push(3);
    let __m = move || __see(&v);
    __m();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
