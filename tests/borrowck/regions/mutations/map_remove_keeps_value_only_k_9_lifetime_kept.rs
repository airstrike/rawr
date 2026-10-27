use std::collections::HashMap;

fn main() {
    let mut k = String::from("k");
    let v = String::from("v");
    let mut m: HashMap<&String, &String> = HashMap::new();
    m.insert(&k, &v);
    let got = m.remove(&&String::from("k")).unwrap();
    let __u = 0u8;
    let __p = __Two(&k, &__u);
    let __k = __p.0;
    k.push_str("!");
    __see(&__k);
    println!("{got} {k}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
