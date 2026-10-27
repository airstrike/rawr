fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let mut best = &0;
    for x in &v {
        best = *x;
    }
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    a += 1;
    __see(&__k);
    println!("{best} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
