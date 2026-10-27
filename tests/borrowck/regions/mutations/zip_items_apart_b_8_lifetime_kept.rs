fn main() {
    let a = 1;
    let mut b = 2;
    let v = vec![&a];
    let w = vec![&b];
    let (x, _) = v.iter().copied().zip(w.iter().copied()).next().unwrap();
    b += 1;
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    println!("{x} {b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
