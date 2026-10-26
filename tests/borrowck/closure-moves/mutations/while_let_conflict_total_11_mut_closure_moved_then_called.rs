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
    let mut __f = || __touch(&mut total);
    let mut __g = __f;
    __g();
    __f();
    println!("{total}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
