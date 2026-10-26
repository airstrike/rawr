fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    c();
    c();
    let __m = move || __see(&c);
    __m();
    let m = n;
    println!("{}", m);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
