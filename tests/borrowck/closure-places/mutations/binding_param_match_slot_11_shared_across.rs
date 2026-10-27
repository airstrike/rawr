fn grow(slot: &mut Option<Vec<i32>>) {
    if let Some(v) = slot {
        let mut push = |x| v.push(x);
        push(1);
        push(2);
    }
}

fn main() {
    let mut slot = Some(vec![0]);
    let __s = &slot;
    grow(&mut slot);
    __see(__s);
    println!("{slot:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
