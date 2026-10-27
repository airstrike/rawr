use std::collections::BTreeMap;

fn main() {
    let mut a = 1;
    let mut m: BTreeMap<i32, &i32> = BTreeMap::new();
    m.insert(1, &a);
    let w: Vec<&i32> = m.values().copied().collect();
    let __u = 0u8;
    let __p = __Two(&__u, &a);
    let __k = __p.0;
    a += 1;
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
