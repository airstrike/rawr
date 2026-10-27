fn main() {
    let mut x = 1;
    let r = &x;
    let __m = move || __see(&x);
    x = 2;
    __m();
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
