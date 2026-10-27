fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __k = Some(0).map(|_| &x);
    println!("{}", x + y);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
