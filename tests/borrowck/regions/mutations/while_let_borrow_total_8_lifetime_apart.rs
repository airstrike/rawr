fn main() {
    let mut stack = vec![1, 2, 3];
    let mut total = 0;
    let r = &mut stack;
    while let Some(x) = r.pop() {
        total += x;
    }
    let __u = 0u8;
    let __p = __Two(&__u, &total);
    let __k = __p.0;
    stack.push(total);
    __see(&__k);
    println!("{:?}", stack);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
