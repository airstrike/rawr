fn main() {
    let mut x = 1;
    let r = &x;
    let __u = 0u8;
    let __p = __Two(&x, &__u);
    let __k = __p.0;
    x = 2;
    __see(&__k);
    println!("{}", *r + x);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
