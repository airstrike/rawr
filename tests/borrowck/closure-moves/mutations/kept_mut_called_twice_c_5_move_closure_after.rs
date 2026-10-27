fn main() {
    let mut v = vec![1, 2];
    let mut c = || v.push(3);
    c();
    c();
    let __m = move || __see(&c);
    __m();
    println!("{}", v.len());
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
