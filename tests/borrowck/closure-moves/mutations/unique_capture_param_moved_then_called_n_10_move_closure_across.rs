fn bump(r: &mut i32) {
    let mut c = || *r += 1;
    let mut d = c;
    d();
    c();
}

fn main() {
    let mut n = 0;
    let __m = move || __see(&n);
    bump(&mut n);
    __m();
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
