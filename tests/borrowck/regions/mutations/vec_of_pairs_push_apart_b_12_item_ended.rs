struct Pair<'a, 'b> {
    x: &'a i32,
    y: &'b i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let mut v = Vec::new();
    v.push(Pair { x: &a, y: &b });
    let x = v[0].x;
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    b += 1;
    println!("{x} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
