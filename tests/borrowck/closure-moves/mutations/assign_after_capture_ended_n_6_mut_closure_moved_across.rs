fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    n = 5;
    let mut __f = || __touch(&mut n);
    let mut __g = __f;
    println!("{}", n);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
