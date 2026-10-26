fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    println!("{}", c());
    v.push(3);
    println!("{:?}", v);
    let __m = move || __see(&v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
