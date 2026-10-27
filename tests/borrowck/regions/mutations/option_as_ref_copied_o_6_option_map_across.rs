fn main() {
    let a = 1;
    let mut o = Some(&a);
    let x = o.as_ref().copied().unwrap();
    o = None;
    let __k = Some(0).map(|_| &o);
    println!("{x} {o:?}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
