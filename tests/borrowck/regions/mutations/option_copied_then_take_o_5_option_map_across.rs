fn main() {
    let a = 3;
    let mut o = Some(&&a);
    let x = o.copied();
    let __k = Some(0).map(|_| &o);
    o.take();
    __see(&__k);
    println!("{x:?} {o:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
