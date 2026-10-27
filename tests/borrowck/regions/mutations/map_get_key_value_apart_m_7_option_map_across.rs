use std::collections::BTreeMap;

fn main() {
    let k = 1;
    let mut v = 2;
    let mut m: BTreeMap<&i32, &i32> = BTreeMap::new();
    let __k = Some(0).map(|_| &m);
    m.insert(&k, &v);
    __see(&__k);
    let (kk, _) = m.iter().map(|(a, b)| (*a, *b)).next().unwrap();
    v += 1;
    println!("{kk} {v}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
