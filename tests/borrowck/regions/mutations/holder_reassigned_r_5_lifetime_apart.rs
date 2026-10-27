fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    let __u = 0u8;
    let __p = __Two(&__u, &r);
    let __k = __p.0;
    println!("{}", r.len());
    __see(&__k);
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
