fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let __u = 0u8;
    let __p = __Two(&__u, &x);
    let __k = __p.0;
    println!("{} {}", x, y);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
