fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut prev = &mut a;
    let mut n = 0;
    while n < 2 {
        b.push(n);
        prev.push(n);
        prev = &mut b;
        n += 1;
    }
    let __k = Some(0).map(|_| &a);
    println!("{:?}", a);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
