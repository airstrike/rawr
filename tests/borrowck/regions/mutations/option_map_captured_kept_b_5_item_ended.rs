fn main() {
    let mut b = 7;
    let o = Some(1);
    let r = o.map(|_| &b);
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    b += 1;
    println!("{r:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
