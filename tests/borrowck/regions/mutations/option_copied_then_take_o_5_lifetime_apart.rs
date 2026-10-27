fn main() {
    let a = 3;
    let mut o = Some(&&a);
    let x = o.copied();
    let __u = 0u8;
    let __p = __Two(&__u, &o);
    let __k = __p.0;
    o.take();
    __see(&__k);
    println!("{x:?} {o:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
