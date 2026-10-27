fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    s.push('b');
    let __u = 0u8;
    let __p = __Two(&s, &__u);
    let __k = __p.0;
    println!("{n} {s}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
