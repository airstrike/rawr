fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    if __c() {
        let __m = move || __see(&n);
        __m();
    }
    n = 5;
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
