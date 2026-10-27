struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let mut v = Vec::new();
    let __u = 0u8;
    let __p = __Two(&__u, &v);
    let __k = __p.0;
    v.push(Pair { x: &a, y: &b });
    __see(&__k);
    let x = v[0].x;
    b += 1;
    println!("{x} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
