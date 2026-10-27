fn main() {
    let mut n = 1;
    let c = move || n + 1;
    n += 10;
    let __h = || &n;
    let __k = __h();
    println!("{} {}", c(), n);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
