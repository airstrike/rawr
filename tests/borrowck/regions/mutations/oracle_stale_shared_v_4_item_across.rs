fn main() {
    let mut v = vec![1];
    let r = &v;
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.push(1);
    __see(&__k);
    println!("{r:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
