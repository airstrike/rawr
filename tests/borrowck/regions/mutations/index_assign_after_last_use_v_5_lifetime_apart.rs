fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v[0] = 5;
    __see(&__k);
    println!("{} {:?}", first, v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
