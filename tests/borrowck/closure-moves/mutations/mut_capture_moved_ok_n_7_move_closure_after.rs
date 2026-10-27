fn main() {
    let mut n = 0;
    let c = || n += 1;
    let mut d = c;
    d();
    d();
    println!("{}", n);
    let __m = move || __see(&n);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
