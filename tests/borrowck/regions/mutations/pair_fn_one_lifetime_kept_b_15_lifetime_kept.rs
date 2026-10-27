struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn second<'a, 'b>(p: &Pair<'a, 'b>) -> &'b i32 {
    p.y
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = Pair { x: &a, y: &b };
    let y = second(&p);
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    b += 1;
    __see(&__k);
    println!("{y}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
