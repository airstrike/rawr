fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.push(3);
    __see(&__k);
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
