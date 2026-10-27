fn main() {
    let a = 3;
    let mut b = 4;
    let v = vec![&a, &b];
    let x = v.iter().copied().last().unwrap();
    b += 1;
    let __u = 0u8;
    let __p = __Two(&b, &__u);
    let __k = __p.0;
    println!("{x} {b}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
