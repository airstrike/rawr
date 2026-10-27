fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    let __k = Some(0).map(|_| &v);
    v.push(&b);
    __see(&__k);
    println!("{x} {}", v.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
