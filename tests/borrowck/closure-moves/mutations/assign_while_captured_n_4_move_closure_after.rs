fn main() {
    let mut n = 1;
    let c = || n + 1;
    n = 5;
    let __m = move || __see(&n);
    __m();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
