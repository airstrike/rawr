fn main() {
    let mut n = 1;
    let c = move || n + 1;
    let __f = || __see(&n);
    let __g = __f;
    __f();
    n += 10;
    __g();
    println!("{} {}", c(), n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
