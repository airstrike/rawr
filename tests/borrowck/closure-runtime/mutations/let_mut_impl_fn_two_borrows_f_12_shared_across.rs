fn make() -> impl Fn() -> usize {
    || 7
}

fn both(a: &dyn Fn() -> usize, b: usize) -> usize {
    a() + b
}

fn main() {
    let mut f = make();
    println!("{}", both(&f, f()));
    let __s = &f;
    f = make();
    __see(__s);
    println!("{}", f());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
