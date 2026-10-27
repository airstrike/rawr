fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.map(|_| &b);
    println!("{r:?}");
    b += 1;
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    println!("{b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
