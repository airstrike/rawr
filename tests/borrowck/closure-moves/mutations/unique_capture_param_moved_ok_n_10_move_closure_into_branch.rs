fn bump(r: &mut i32) {
    let c = || *r += 1;
    let mut d = c;
    d();
    d();
}

fn main() {
    let mut n = 0;
    if __c() {
        let __m = move || __see(&n);
        __m();
    }
    bump(&mut n);
    println!("{}", n);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
