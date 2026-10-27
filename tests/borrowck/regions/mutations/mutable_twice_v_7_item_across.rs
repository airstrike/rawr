fn main() {
    let mut v = vec![1];
    let a = &mut v;
    let b = &mut v;
    b.push(2);
    a.push(3);
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    println!("{:?}", v);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
