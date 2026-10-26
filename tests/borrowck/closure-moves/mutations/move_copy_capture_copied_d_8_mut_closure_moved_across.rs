fn main() {
    let mut n = 1;
    let mut c = move || {
        n += 1;
        n
    };
    let mut d = c;
    let mut __f = || __touch(&mut d);
    let mut __g = __f;
    println!("{} {} {}", d(), c(), n);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
