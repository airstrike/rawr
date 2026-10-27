fn main() {
    let mut slot = Some(vec![1]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(2);
        let mut g = || v.push(3);
        f();
        g();
    }
    let __b = &mut slot;
    println!("{slot:?}");
    if __c() {
        __touch(__b);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
