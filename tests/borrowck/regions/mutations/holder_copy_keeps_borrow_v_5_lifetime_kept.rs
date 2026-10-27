fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __u = 0u8;
    let __p = __Two(&v, &__u);
    let __k = __p.0;
    v.push(2);
    __see(&__k);
    println!("{}", s.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
