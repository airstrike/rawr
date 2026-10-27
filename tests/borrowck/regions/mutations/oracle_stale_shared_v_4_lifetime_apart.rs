fn main() {
    let mut v = vec![1];
    let r = &v;
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.push(1);
    __see(&__k);
    println!("{r:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
