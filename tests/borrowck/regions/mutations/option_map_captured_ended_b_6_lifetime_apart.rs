fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.map(|_| &b);
    println!("{r:?}");
    let __u = 0u8;
    let __p = __Two(&__u, &b);
    let __k = __p.0;
    b += 1;
    __see(&__k);
    println!("{b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
