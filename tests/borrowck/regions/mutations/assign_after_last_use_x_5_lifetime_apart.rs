fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __u = 0u8;
    let __p = __Two(&__u, &x);
    let __k = __p.0;
    x = 2;
    __see(&__k);
    println!("{}", x + y);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
