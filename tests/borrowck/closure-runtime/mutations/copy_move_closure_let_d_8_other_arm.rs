fn main() {
    let mut n = 1;
    let mut c = move || {
        n += 1;
        n
    };
    let mut d = c;
    let __b = &mut d;
    if __c() {
        __touch(__b);
    } else {
        println!("{} {} {} {}", d(), d(), c(), n);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
