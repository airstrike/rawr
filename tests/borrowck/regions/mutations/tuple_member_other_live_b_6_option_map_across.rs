fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let y = t.1;
    let __k = Some(0).map(|_| &b);
    b += 1;
    __see(&__k);
    println!("{y} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
