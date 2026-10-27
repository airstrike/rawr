use std::collections::BTreeMap;

fn main() {
    let k = 1;
    let mut v = 2;
    let mut m: BTreeMap<&i32, &i32> = BTreeMap::new();
    m.insert(&k, &v);
    let (kk, _) = m.iter().map(|(a, b)| (*a, *b)).next().unwrap();
    v += 1;
    println!("{kk} {v}");
}
