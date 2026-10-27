struct S {
    a: i64,
    b: i64,
}
fn main() {
    let mut s = S { a: 1, b: 2 };
    let r = &mut s.a;
    s.a += 10;
    *r += 100;
    let __v = vec![&s];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{} {}", s.a, s.b);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
