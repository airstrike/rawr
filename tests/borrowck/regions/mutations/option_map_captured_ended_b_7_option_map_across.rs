fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.map(|_| &b);
    println!("{r:?}");
    b += 1;
    let __k = Some(0).map(|_| &b);
    println!("{b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
