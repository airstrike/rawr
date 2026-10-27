use std::collections::BTreeMap;

fn main() {
    let a = 1;
    let mut m: BTreeMap<i32, &i32> = BTreeMap::new();
    let __u = 0u8;
    let __p = __Two(&m, &__u);
    let __k = __p.0;
    m.insert(1, &a);
    __see(&__k);
    let w: Vec<&i32> = m.values().copied().collect();
    m.clear();
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
