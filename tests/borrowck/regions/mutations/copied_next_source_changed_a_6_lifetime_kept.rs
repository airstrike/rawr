fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    let __u = 0u8;
    let __p = __Two(&a, &__u);
    let __k = __p.0;
    a.push_str("!");
    __see(&__k);
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
