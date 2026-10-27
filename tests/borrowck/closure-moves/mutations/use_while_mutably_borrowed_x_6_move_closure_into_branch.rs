fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    if __c() {
        let __m = move || __see(&x);
        __m();
    }
    println!("{} {}", x, y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
