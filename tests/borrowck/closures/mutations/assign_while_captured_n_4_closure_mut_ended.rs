fn main() {
    let mut n = 1;
    let c = || n + 1;
    let mut __f = || __touch(&mut n);
    __f();
    n = 5;
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
