fn main() {
    let a = 3;
    let mut v = vec![&a];
    let (i, x) = v.iter().copied().enumerate().next().unwrap();
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.clear();
    __see(&__k);
    println!("{i} {x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
