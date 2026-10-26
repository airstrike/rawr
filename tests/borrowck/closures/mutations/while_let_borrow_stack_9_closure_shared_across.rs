fn main() {
    let mut stack = vec![1, 2, 3];
    let mut total = 0;
    let r = &mut stack;
    while let Some(x) = r.pop() {
        total += x;
    }
    stack.push(total);
    let __g = || __see(&stack);
    println!("{:?}", stack);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
