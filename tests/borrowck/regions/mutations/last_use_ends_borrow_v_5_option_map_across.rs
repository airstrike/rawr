fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    let __k = Some(0).map(|_| &v);
    v.push(3);
    __see(&__k);
    let m = &mut v;
    m.push(4);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
