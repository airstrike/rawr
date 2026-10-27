fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.push(2);
    println!("{}", s.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
