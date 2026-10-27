use std::collections::BTreeMap;

fn main() {
    let mut a = 1;
    let mut m: BTreeMap<i32, &i32> = BTreeMap::new();
    m.insert(1, &a);
    let w: Vec<&i32> = m.values().copied().collect();
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
