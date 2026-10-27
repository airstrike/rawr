fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    let __k = Some(0).map(|_| &v);
    println!("{:?}", v);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
