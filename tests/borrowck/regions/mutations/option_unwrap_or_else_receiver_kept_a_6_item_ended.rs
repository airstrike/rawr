fn main() {
    let mut a = 1;
    let b = 2;
    let o = Some(&a);
    let r = o.unwrap_or_else(|| &b);
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
