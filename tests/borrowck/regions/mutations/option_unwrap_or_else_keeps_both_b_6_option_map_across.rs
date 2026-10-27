fn main() {
    let a = 1;
    let mut b = 2;
    let o = Some(&a);
    let r = o.unwrap_or_else(|| &b);
    let __k = Some(0).map(|_| &b);
    b += 1;
    __see(&__k);
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
