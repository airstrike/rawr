use std::collections::HashMap;

fn main() {
    let a = 1;
    let mut m: HashMap<i32, &i32> = HashMap::new();
    let __k = Some(0).map(|_| &m);
    m.insert(1, &a);
    __see(&__k);
    let got = m.get(&1).unwrap();
    m.insert(2, &a);
    println!("{got}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);
