fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    let __h = || &n;
    let __k = __h();
    n = 5;
    __see(__k);
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
