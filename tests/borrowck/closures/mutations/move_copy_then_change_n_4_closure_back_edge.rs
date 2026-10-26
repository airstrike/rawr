fn main() {
    let mut n = 1;
    let c = move || n + 1;
    let mut __f = || __touch(&mut n);
    for __i in 0..2 {
        __f();
        n += 10;
    }
    println!("{} {}", c(), n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
