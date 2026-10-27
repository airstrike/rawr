fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x: String = v.iter().copied().next().unwrap().clone();
    v.push(&a);
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{x} {}", v.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
