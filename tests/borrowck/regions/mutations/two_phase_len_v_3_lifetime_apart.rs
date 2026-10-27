fn main() {
    let mut v = vec![1, 2];
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.push(v.len());
    __see(&__k);
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
