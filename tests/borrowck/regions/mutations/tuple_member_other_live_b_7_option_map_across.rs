fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let y = t.1;
    b += 1;
    let __k = Some(0).map(|_| &b);
    println!("{y} {b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
