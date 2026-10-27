struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    let __k = Some(0).map(|_| &s);
    s.b += 10;
    __see(&__k);
    *r += 100;
    println!("{} {}", s.a, s.b);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
