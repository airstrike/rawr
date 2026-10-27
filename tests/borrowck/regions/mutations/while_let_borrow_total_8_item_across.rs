fn main() {
    let mut stack = vec![1, 2, 3];
    let mut total = 0;
    let r = &mut stack;
    while let Some(x) = r.pop() {
        total += x;
    }
    let __v = vec![&total];
    let __k = __v.iter().copied().next().unwrap();
    stack.push(total);
    __see(&__k);
    println!("{:?}", stack);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
