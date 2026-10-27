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
    let __b = &mut next;
    println!("{}", next());
    if __c() {
        __touch(__b);
    }
    let _ = peek;
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
