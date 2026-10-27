struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let b = 2;
    let mut c = 3;
    let mut p = Pair { x: &a, y: &b };
    let __u = 0u8;
    let __p = __Two(&c, &__u);
    let __k = __p.0;
    p.y = &c;
    __see(&__k);
    let y = p.y;
    c += 1;
    println!("{y}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
