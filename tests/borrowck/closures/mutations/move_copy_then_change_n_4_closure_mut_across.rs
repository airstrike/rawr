fn main() {
    let mut n = 1;
    let c = move || n + 1;
    let mut __f = || __touch(&mut n);
    n += 10;
    __f();
    println!("{} {}", c(), n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
