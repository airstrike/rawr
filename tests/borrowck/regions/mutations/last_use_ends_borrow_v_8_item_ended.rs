fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let n = r.len();
    v.push(3);
    let m = &mut v;
    m.push(4);
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.push(5);
    println!("{n} {:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
