struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = Pair { x: &a, y: &b };
    let __k = Some(0).map(|_| &b);
    b += 1;
    __see(&__k);
    println!("{}", p.x);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
