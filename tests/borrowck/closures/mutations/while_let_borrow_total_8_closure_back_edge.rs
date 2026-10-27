fn main() {
    let mut stack = vec![1, 2, 3];
    let mut total = 0;
    let r = &mut stack;
    while let Some(x) = r.pop() {
        total += x;
    }
    let mut __f = || __touch(&mut total);
    for __i in 0..2 {
        __f();
        stack.push(total);
    }
    println!("{:?}", stack);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
