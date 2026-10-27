fn grow(slot: &mut Option<Vec<i32>>) {
    if let Some(v) = slot {
        let mut push = |x| v.push(x);
        push(1);
        for __i in 0..2 {
            let __b = &mut push;
            __touch(__b);
            push(2);
        }
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
