fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __u = 0u8;
    let __p = __Two(&s, &__u);
    let __k = __p.0;
    s.push('b');
    __see(&__k);
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
