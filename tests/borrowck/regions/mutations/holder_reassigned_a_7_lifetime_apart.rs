fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    r = &b;
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    a.push(3);
    __see(&__k);
    println!("{} {:?}", r.len(), a);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
