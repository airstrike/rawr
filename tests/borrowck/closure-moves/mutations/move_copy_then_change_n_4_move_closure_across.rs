fn main() {
    let mut n = 1;
    let c = move || n + 1;
    let __m = move || __see(&n);
    n += 10;
    __m();
    println!("{} {}", c(), n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
