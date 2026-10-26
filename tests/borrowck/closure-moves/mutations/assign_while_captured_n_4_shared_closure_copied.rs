fn main() {
    let mut n = 1;
    let c = || n + 1;
    let __f = || __see(&n);
    let __g = __f;
    __f();
    n = 5;
    __g();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
