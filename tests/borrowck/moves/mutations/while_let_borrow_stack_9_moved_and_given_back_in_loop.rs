fn main() {
    let mut stack = vec![1, 2, 3];
    let mut total = 0;
    let r = &mut stack;
    while let Some(x) = r.pop() {
        total += x;
    }
    stack.push(total);
    for __i in 0..2 {
        let __m = stack;
        stack = __m;
    }
    println!("{:?}", stack);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
