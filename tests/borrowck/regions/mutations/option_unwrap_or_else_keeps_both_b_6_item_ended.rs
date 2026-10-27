fn main() {
    let a = 1;
    let mut b = 2;
    let o = Some(&a);
    let r = o.unwrap_or_else(|| &b);
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    b += 1;
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
