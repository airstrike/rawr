fn main() {
    let a = 1;
    let mut o = Some(&a);
    let x = o.as_ref().copied().unwrap();
    o = None;
    let __u = 0u8;
    let __p = __Two(&__u, &o);
    let __k = __p.0;
    println!("{x} {o:?}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
