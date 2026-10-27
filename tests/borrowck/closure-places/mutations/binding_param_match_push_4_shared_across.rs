fn grow(slot: &mut Option<Vec<i32>>) {
    if let Some(v) = slot {
        let mut push = |x| v.push(x);
        let __s = &push;
        push(1);
        __see(__s);
        push(2);
    }
}

fn main() {
    let mut slot = Some(vec![0]);
    grow(&mut slot);
    println!("{slot:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
