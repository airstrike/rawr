fn main() {
    let mut a = vec![1];
    let mut b = vec![2];
    let mut prev = &mut a;
    let mut n = 0;
    while n < 2 {
        prev.push(n);
        let r = &mut b;
        r.push(n);
        prev = r;
        n += 1;
    }
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    b.push(9);
    __see(&__k);
    println!("{:?} {:?}", a, b);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
