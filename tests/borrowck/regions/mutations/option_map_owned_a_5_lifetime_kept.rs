fn main() {
    let mut a = String::from("a");
    let o = Some(&a);
    let n = o.map(|s| s.len());
    let __u = 0u8;
    let __p = __Two(&a, &__u);
    let __k = __p.0;
    a.push_str("!");
    __see(&__k);
    println!("{n:?} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
