fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v[0] = 5;
    println!("{} {:?}", first, v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
