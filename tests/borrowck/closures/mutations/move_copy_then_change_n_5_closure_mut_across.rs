fn main() {
    let mut n = 1;
    let c = move || n + 1;
    n += 10;
    let mut __f = || __touch(&mut n);
    println!("{} {}", c(), n);
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
