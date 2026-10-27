fn grow(slot: &mut Option<Vec<i32>>) {
    if let Some(v) = slot {
        let mut push = |x| v.push(x);
        push(1);
        push(2);
    }
}

fn main() {
    let mut slot = Some(vec![0]);
    let __b = &mut slot;
    for __i in 0..2 {
        __touch(__b);
        grow(&mut slot);
    }
    println!("{slot:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
