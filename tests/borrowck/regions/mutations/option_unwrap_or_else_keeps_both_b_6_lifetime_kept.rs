fn main() {
    let a = 1;
    let mut b = 2;
    let o = Some(&a);
    let r = o.unwrap_or_else(|| &b);
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    b += 1;
    __see(&__k);
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
