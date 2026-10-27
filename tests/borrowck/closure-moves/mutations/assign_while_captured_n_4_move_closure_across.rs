fn main() {
    let mut n = 1;
    let c = || n + 1;
    let __m = move || __see(&n);
    n = 5;
    __m();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
