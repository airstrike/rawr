fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let __k = Some(0).map(|_| &v);
    r.push(v.len());
    __see(&__k);
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
