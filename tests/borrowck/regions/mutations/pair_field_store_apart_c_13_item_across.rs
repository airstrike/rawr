struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let b = 2;
    let mut c = 3;
    let mut p = Pair { x: &a, y: &b };
    p.y = &c;
    let x = p.x;
    let __v = vec![&c];
    let __k = __v.iter().copied().next().unwrap();
    c += 1;
    __see(&__k);
    println!("{x} {c}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
