fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let mut best = &&0;
    for x in &v {
        best = x;
    }
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.clear();
    __see(&__k);
    println!("{best}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
