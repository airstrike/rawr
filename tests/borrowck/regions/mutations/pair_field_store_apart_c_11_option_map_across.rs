struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let b = 2;
    let mut c = 3;
    let mut p = Pair { x: &a, y: &b };
    let __k = Some(0).map(|_| &c);
    p.y = &c;
    __see(&__k);
    let x = p.x;
    c += 1;
    println!("{x} {c}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
