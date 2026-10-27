fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    let __k = Some(0).map(|_| &r);
    r = &b;
    __see(&__k);
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
