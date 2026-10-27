use std::collections::HashMap;

fn main() {
    let mut k = String::from("k");
    let v = String::from("v");
    let mut m: HashMap<&String, &String> = HashMap::new();
    let __u = 0u8;
    let __p = __Two(&k, &__u);
    let __k = __p.0;
    m.insert(&k, &v);
    __see(&__k);
    let got = m.remove(&&String::from("k")).unwrap();
    k.push_str("!");
    println!("{got} {k}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
