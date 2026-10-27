fn main() {
    let a = 1;
    let mut o = Some(&a);
    let x = o.as_ref().copied().unwrap();
    let __u = 0u8;
    let __p = __Two(&o, &__u);
    let __k = __p.0;
    o = None;
    __see(&__k);
    println!("{x} {o:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
