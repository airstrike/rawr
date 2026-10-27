fn main() {
    let mut n = 1;
    let mut c = move || {
        n += 1;
        n
    };
    let mut d = c;
    for __i in 0..2 {
        let __b = &mut n;
        __touch(__b);
        println!("{} {} {} {}", d(), d(), c(), n);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
