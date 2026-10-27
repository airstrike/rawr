fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    let __k = Some(0).map(|_| &v);
    v.push(3);
    __see(&__k);
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
