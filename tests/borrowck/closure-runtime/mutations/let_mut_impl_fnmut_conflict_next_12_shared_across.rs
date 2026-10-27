fn counter() -> impl FnMut() -> i32 {
    let mut n = 0;
    move || {
        n += 1;
        n
    }
}

fn main() {
    let mut next = counter();
    let peek = &next;
    let __s = &next;
    println!("{}", next());
    __see(__s);
    let _ = peek;
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
