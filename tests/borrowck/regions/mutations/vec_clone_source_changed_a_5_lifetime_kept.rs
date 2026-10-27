fn main() {
    let mut a = 1;
    let v = vec![&a];
    let w = v.clone();
    let __u = 0u8;
    let __p = __Two(&a, &__u);
    let __k = __p.0;
    a += 1;
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
