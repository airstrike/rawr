fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __m = move || __see(&x);
    x = 2;
    __m();
    println!("{}", x + y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
