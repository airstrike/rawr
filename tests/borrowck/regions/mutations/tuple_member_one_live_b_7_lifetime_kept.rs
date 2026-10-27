fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let x = t.0;
    b += 1;
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    println!("{x} {b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
