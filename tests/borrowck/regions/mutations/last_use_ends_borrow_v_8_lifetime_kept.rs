fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    v.push(3);
    let m = &mut v;
    m.push(4);
    let __u = 0u8;
    let __p = __Two(&v, &__u);
    let __k = __p.0;
    v.push(5);
    __see(&__k);
    println!("{n} {:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
