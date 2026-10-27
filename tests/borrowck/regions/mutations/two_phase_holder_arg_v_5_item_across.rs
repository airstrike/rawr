fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    println!("{:?}", v);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
