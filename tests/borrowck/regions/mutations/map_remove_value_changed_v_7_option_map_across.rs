use std::collections::HashMap;

fn main() {
    let k = String::from("k");
    let mut v = String::from("v");
    let mut m: HashMap<&String, &String> = HashMap::new();
    let __k = Some(0).map(|_| &v);
    m.insert(&k, &v);
    __see(&__k);
    let got = m.remove(&&String::from("k")).unwrap();
    v.push_str("!");
    println!("{got}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
