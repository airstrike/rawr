fn main() {
    let mut v = vec![1];
    let mut keep = &v;
    let mut n = 0;
    while n < 3 {
        v.push(n);
        keep = &v;
        n += 1;
    }
    let __v = vec![&keep];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{:?}", keep);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
