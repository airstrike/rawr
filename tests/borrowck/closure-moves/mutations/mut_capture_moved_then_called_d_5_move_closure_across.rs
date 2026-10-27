fn main() {
    let mut n = 0;
    let mut c = || n += 1;
    let mut d = c;
    let __m = move || __see(&d);
    d();
    __m();
    c();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
