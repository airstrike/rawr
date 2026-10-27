fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __u = 0u8;
    let __p = __Two(&v, &__u);
    let __k = __p.0;
    r.push(v.len());
    __see(&__k);
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
