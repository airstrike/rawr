fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x: String = v.iter().copied().next().unwrap().clone();
    let __k = Some(0).map(|_| &v);
    v.push(&a);
    __see(&__k);
    println!("{x} {}", v.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
