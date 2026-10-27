fn main() {
    let mut n = 1;
    let mut c = move || {
        n += 1;
        n
    };
    let mut d = c;
    let __m = move || __see(&n);
    println!("{} {} {}", d(), c(), n);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
