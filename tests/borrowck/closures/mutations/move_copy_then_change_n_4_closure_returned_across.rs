fn main() {
    let mut n = 1;
    let c = move || n + 1;
    let __h = || &n;
    let __k = __h();
    n += 10;
    __see(__k);
    println!("{} {}", c(), n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
