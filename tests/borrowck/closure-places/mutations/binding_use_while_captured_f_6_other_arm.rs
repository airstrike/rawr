fn main() {
    let mut slot = Some(vec![1]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(2);
        v.push(9);
        let __b = &mut f;
        if __c() {
            __touch(__b);
        } else {
            f();
        }
    }
    println!("{slot:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
