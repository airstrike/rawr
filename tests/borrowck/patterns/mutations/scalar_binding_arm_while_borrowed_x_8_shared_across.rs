fn main() {
    let mut x = 1;
    let r = &mut x;
    let n = match x {
        n => n + 1,
    };
    *r += 1;
    let __s = &x;
    println!("{} {}", n, x);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
