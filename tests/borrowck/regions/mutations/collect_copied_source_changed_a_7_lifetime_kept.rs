fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let w: Vec<&i32> = v.iter().copied().collect();
    a += 1;
    let __u = 0u8;
    let __p = __Two(&a, &__u);
    let __k = __p.0;
    println!("{w:?} {a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
