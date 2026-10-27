fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let (x, _) = t;
    b += 1;
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{x} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
