fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __k = Some(0).map(|_| &x);
    x = 2;
    __see(&__k);
    println!("{}", x + y);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
