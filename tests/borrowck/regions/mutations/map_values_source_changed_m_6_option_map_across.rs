use std::collections::BTreeMap;

fn main() {
    let mut a = 1;
    let mut m: BTreeMap<i32, &i32> = BTreeMap::new();
    let __k = Some(0).map(|_| &m);
    m.insert(1, &a);
    __see(&__k);
    let w: Vec<&i32> = m.values().copied().collect();
    a += 1;
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
