fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let (_, y) = t;
    let __u = 0u8;
    let __p = __Two(&__u, &b);
    let __k = __p.0;
    b += 1;
    __see(&__k);
    println!("{y}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
