fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a.push_str("!");
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
