fn main() {
    let mut v = vec![1, 2];
    let mut total = 0;
    for x in &v {
        total += *x;
    }
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.push(total);
    __see(&__k);
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
