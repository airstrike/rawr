fn main() {
    let mut v = vec![1, 2];
    let __k = Some(0).map(|_| &v);
    v[v.len() - 1] = 5;
    __see(&__k);
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
