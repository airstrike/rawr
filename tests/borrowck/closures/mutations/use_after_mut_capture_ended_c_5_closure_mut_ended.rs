fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    c();
    let mut __f = || __touch(&mut c);
    __f();
    c();
    let m = n;
    println!("{}", m);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
