fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __u = 0u8;
    let __p = __Two(&x, &__u);
    let __k = __p.0;
    println!("{}", *r + x);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
