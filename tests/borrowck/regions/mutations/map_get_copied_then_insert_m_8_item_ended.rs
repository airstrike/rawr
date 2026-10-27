use std::collections::HashMap;

fn main() {
    let a = 1;
    let mut m: HashMap<i32, &i32> = HashMap::new();
    m.insert(1, &a);
    let got = m.get(&1).copied().unwrap();
    let __v = vec![&m];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    m.insert(2, &a);
    println!("{got}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
