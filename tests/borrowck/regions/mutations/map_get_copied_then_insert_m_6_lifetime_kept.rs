use std::collections::HashMap;

fn main() {
    let a = 1;
    let mut m: HashMap<i32, &i32> = HashMap::new();
    let __u = 0u8;
    let __p = __Two(&m, &__u);
    let __k = __p.0;
    m.insert(1, &a);
    __see(&__k);
    let got = m.get(&1).copied().unwrap();
    m.insert(2, &a);
    println!("{got}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
