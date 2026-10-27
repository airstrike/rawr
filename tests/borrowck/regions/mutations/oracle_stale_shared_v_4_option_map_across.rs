fn main() {
    let mut v = vec![1];
    let r = &v;
    let __k = Some(0).map(|_| &v);
    v.push(1);
    __see(&__k);
    println!("{r:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
