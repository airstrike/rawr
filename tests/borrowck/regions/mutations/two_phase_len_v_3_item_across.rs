fn main() {
    let mut v = vec![1, 2];
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.push(v.len());
    __see(&__k);
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
