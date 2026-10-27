struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn make<'a, 'b>(x: &'a i32, y: &'b i32) -> Pair<'a, 'b> {
    Pair { x, y }
}

fn main() {
    let mut a = 1;
    let b = 2;
    let p = make(&a, &b);
    let x = p.x;
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    a += 1;
    __see(&__k);
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
