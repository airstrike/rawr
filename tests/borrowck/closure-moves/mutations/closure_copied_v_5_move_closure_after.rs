fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let d = c;
    v.push(1);
    let __m = move || __see(&v);
    __m();
    println!("{}", d());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
