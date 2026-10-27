struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = Pair { x: &a, y: &b };
    let y = p.y;
    b += 1;
    let __u = 0u8;
    let __p = __Two(&__u, &b);
    let __k = __p.0;
    println!("{y} {b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
