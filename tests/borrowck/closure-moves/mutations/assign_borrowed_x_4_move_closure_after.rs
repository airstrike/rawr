fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __m = move || __see(&x);
    __m();
    println!("{}", *r + x);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
