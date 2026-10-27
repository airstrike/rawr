fn main() {
    let mut stack = vec![1, 2, 3];
    let mut total = 0;
    let r = &mut stack;
    while let Some(x) = r.pop() {
        total += x;
        if total > 100 {
            stack.clear();
        }
    }
    if __c() {
        let __m = move || __see(&total);
        __m();
    }
    println!("{total}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
