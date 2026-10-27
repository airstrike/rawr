fn main() {
    let mut n = 1;
    let mut c = || n += 1;
    if __c() {
        let __m = move || __see(&c);
        __m();
    }
    c();
    c();
    let m = n;
    println!("{}", m);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
