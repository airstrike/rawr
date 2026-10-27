fn main() {
    let mut v = vec![1, 2];
    let mut total = 0;
    for x in &v {
        total += *x;
    }
    let __v = vec![&total];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.push(total);
    println!("{:?}", v);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
