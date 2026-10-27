fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    println!("{}", x + y);
    let __m = move || __see(&x);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
