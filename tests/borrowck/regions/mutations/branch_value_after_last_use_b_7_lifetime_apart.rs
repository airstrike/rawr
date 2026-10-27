fn main() {
    let a = vec![1];
    let mut b = vec![2];
    let c = a.len() > 0;
    let r = if c { &a } else { &b };
    println!("{}", r.len());
    let __u = 0u8;
    let __p = __Two(&__u, &b);
    let __k = __p.0;
    b.push(3);
    __see(&__k);
    println!("{:?}", b);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
