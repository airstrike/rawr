fn main() {
    let mut a = String::from("a");
    let o = Some(&a);
    let n = o.map(|s| s.len());
    a.push_str("!");
    let __u = 0u8;
    let __p = __Two(&a, &__u);
    let __k = __p.0;
    println!("{n:?} {a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
