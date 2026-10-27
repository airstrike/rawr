fn main() {
    let mut slot = Some(vec![1]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(2);
        f();
        let __s = &f;
        f();
        __see(__s);
    }
    println!("{slot:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
