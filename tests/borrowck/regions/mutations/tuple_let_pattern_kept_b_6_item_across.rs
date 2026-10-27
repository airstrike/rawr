fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let (_, y) = t;
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    b += 1;
    __see(&__k);
    println!("{y}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
