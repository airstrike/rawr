struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn second<'a, 'b>(p: &Pair<'a, 'b>) -> &'b i32 {
    p.y
}

fn main() {
    let mut a = 1;
    let b = 2;
    let p = Pair { x: &a, y: &b };
    let y = second(&p);
    a += 1;
    let __k = Some(0).map(|_| &a);
    println!("{y} {a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
