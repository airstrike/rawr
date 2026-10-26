fn main() {
    let mut n = 1;
    let c = move || n + 1;
    n += 10;
    let mut __f = || __touch(&mut n);
    let mut __g = __f;
    println!("{} {}", c(), n);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
