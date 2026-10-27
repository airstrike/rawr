use std::collections::BTreeMap;

fn main() {
    let k = 1;
    let mut v = 2;
    let mut m: BTreeMap<&i32, &i32> = BTreeMap::new();
    m.insert(&k, &v);
    let (kk, _) = m.iter().map(|(a, b)| (*a, *b)).next().unwrap();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v += 1;
    println!("{kk} {v}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
