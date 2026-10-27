fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.map(|_| &b);
    let __k = Some(0).map(|_| &b);
    b += 1;
    __see(&__k);
    println!("{r:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
