fn bump(r: &mut i32) {
    let mut c = || *r += 1;
    let mut d = c;
    d();
    let __m = move || __see(&c);
    c();
    __m();
}

fn main() {
    let mut n = 0;
    bump(&mut n);
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
