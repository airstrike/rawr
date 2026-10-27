fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    println!("{x}");
    let __k = Some(0).map(|_| &a);
    a.push_str("!");
    __see(&__k);
    println!("{a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
