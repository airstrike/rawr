fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    let __g = || __see(&n);
    n = 5;
    __g();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
