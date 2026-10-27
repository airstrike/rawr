fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    let mut __f = || __touch(&mut n);
    let mut __g = __f;
    __g();
    __f();
    n = 5;
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
