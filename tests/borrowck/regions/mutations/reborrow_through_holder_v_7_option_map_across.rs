fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let s = &mut *r;
    s.push(2);
    r.push(3);
    let __k = Some(0).map(|_| &v);
    println!("{:?}", v);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
