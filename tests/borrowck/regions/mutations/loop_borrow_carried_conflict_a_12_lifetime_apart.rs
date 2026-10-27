fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut prev = &mut a;
    let mut n = 0;
    while n < 2 {
        b.push(n);
        prev.push(n);
        prev = &mut b;
        n += 1;
    }
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    println!("{:?}", a);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
