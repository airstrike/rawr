fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    r.push(v.len());
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
