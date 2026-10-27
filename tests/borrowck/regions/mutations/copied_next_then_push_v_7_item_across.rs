fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    v.push(&b);
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    println!("{x} {}", v.len());
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
