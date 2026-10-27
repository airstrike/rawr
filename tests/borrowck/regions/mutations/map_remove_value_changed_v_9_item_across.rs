use std::collections::HashMap;

fn main() {
    let k = String::from("k");
    let mut v = String::from("v");
    let mut m: HashMap<&String, &String> = HashMap::new();
    m.insert(&k, &v);
    let got = m.remove(&&String::from("k")).unwrap();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.push_str("!");
    __see(&__k);
    println!("{got}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
