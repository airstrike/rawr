fn main() {
    let mut n = 0;
    let r = &mut n;
    let mut c = || *r += 1;
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
