fn main() {
    let mut slot = Some(vec![1, 2]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(3);
        f();
    }
    let __s = &slot;
    println!("{slot:?}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
