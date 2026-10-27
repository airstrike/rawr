fn main() {
    let mut v = vec![1];
    let empty = v.is_empty();
    let r = &mut v;
    if empty {
        v.push(1);
        panic!("empty");
    }
    r.push(2);
    let __k = Some(0).map(|_| &v);
    println!("{:?}", v);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
