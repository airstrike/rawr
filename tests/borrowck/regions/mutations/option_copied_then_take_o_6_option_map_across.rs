fn main() {
    let a = 3;
    let mut o = Some(&&a);
    let x = o.copied();
    o.take();
    let __k = Some(0).map(|_| &o);
    println!("{x:?} {o:?}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
