fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    n = 5;
    let __h = || &n;
    let __k = __h();
    println!("{}", n);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
