fn main() {
    let mut n = 1;
    let c = || n + 1;
    println!("{}", c());
    n = 5;
    let mut __f = || __touch(&mut n);
    println!("{}", n);
    if __c() {
        __f();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
