fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v[0] = 5;
    __see(&__k);
    println!("{} {:?}", first, v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
