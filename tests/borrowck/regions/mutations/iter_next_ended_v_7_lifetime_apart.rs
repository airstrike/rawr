fn main() {
    let a = 1;
    let mut v = vec![&a];
    let x = v.iter().next().unwrap();
    println!("{x}");
    v.push(&a);
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    println!("{}", v.len());
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
