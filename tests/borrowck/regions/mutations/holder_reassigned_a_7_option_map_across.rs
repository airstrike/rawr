fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    r = &b;
    let __k = Some(0).map(|_| &a);
    a.push(3);
    __see(&__k);
    println!("{} {:?}", r.len(), a);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
