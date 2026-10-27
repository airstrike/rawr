fn main() {
    let mut slot = Some(vec![1]);
    if let Some(v) = &mut slot {
        let mut f = || v.push(2);
        let mut g = || v.push(3);
        f();
        let __b = &mut g;
        for __i in 0..2 {
            __touch(__b);
            g();
        }
    }
    println!("{slot:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
