fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x: String = v.iter().copied().next().unwrap().clone();
    v.push(&a);
    let __u = 0u8;
    let __p = __Two(&v, &__u);
    let __k = __p.0;
    println!("{x} {}", v.len());
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
