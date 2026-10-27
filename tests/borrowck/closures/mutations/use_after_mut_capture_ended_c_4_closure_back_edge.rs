fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    let mut __f = || __touch(&mut c);
    for __i in 0..2 {
        __f();
        c();
    }
    c();
    let m = n;
    println!("{}", m);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
