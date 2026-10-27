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
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
