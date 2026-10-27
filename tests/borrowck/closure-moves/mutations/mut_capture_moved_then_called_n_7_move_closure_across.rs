fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    let mut d = c;
    d();
    c();
    let __m = move || __see(&n);
    println!("{}", n);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
