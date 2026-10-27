struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn make<'a, 'b>(x: &'a i32, y: &'b i32) -> Pair<'a, 'b> {
    Pair { x, y }
}

fn main() {
    let a = 1;
    let mut b = 2;
    let p = make(&a, &b);
    let x = p.x;
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    b += 1;
    __see(&__k);
    println!("{x} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
