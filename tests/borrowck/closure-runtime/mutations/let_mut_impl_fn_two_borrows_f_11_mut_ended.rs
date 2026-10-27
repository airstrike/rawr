fn make() -> impl Fn() -> usize {
    || 7
}

fn both(a: &dyn Fn() -> usize, b: usize) -> usize {
    a() + b
}

fn main() {
    let mut f = make();
    let __b = &mut f;
    __touch(__b);
    println!("{}", both(&f, f()));
    f = make();
    println!("{}", f());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
