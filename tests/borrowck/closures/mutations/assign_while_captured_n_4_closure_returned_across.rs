fn main() {
    let mut n = 1;
    let c = || n + 1;
    let __h = || &n;
    let __k = __h();
    n = 5;
    __see(__k);
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
