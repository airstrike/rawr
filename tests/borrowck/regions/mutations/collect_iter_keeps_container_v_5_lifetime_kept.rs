fn main() {
    let a = 3;
    let mut v = vec![&a];
    let w: Vec<&&i32> = v.iter().collect();
    let __u = 0u8;
    let __p = __Two(&v, &__u);
    let __k = __p.0;
    v.clear();
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
