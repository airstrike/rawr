fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let __k = Some(0).map(|_| &v);
    println!("{:?}", v);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
