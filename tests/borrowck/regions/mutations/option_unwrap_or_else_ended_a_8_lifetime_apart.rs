fn main() {
    let mut a = 1;
    let b = 2;
    let o = Some(&a);
    let r = o.unwrap_or_else(|| &b);
    println!("{r}");
    a += 1;
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    println!("{a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
