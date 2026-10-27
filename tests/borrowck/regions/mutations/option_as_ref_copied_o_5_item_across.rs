fn main() {
    let a = 1;
    let mut o = Some(&a);
    let x = o.as_ref().copied().unwrap();
    let __v = vec![&o];
    let __k = __v.iter().copied().next().unwrap();
    o = None;
    __see(&__k);
    println!("{x} {o:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
