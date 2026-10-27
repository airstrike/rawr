fn main() {
    let mut v = vec![1, 2];
    let mut c = || v[0] += 1;
    println!("{}", v[1]);
    let mut __f = || __touch(&mut c);
    for __i in 0..2 {
        __f();
        c();
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
